//! Graph node registration and frame dispatch helpers.

use crate::sys;
use core::cell::UnsafeCell;
use core::ffi::{c_char, CStr};
use core::marker::PhantomData;

pub const FRAME_SIZE: u32 = sys::VLIB_FRAME_SIZE;
pub const FLAG_TRACE: u16 = sys::VLIB_NODE_FLAG_TRACE as u16;

pub type NodeFn = unsafe extern "C" fn(
    vm: *mut sys::vlib_main_t,
    node: *mut sys::vlib_node_runtime_t,
    frame: *mut sys::vlib_frame_t,
) -> sys::uword;

#[derive(Clone, Copy)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

/// One error/counter definition: (counter-name, display-description).
pub struct ErrorDef(pub &'static CStr, pub &'static CStr, pub Severity);

/// Storage for a node registration plus its next-node string table
/// (`vlib_node_registration_t` ends in a flexible array member).
#[repr(C)]
pub struct NodeReg<const N_NEXT: usize> {
    reg: sys::vlib_node_registration_t,
    next_nodes: [*const c_char; N_NEXT],
}

pub struct NodeCell<const N_NEXT: usize>(UnsafeCell<NodeReg<N_NEXT>>);
unsafe impl<const N: usize> Sync for NodeCell<N> {}

impl<const N: usize> NodeCell<N> {
    pub const fn new() -> Self {
        NodeCell(UnsafeCell::new(unsafe {
            core::mem::MaybeUninit::zeroed().assume_init()
        }))
    }

    /// Node index assigned by VPP (valid after graph init).
    pub fn index(&self) -> u32 {
        unsafe { (*self.0.get()).reg.index }
    }
}

/// Fill in and link a node registration. Call from a `vpp::ctor!`
/// constructor — i.e. at plugin dlopen time, before VPP's node graph init.
///
/// # Safety
/// Must run at dlopen time on the single loading thread.
pub unsafe fn register_internal_node<const N: usize>(
    cell: &'static NodeCell<N>,
    name: &'static CStr,
    function: NodeFn,
    format_trace: sys::format_function_t,
    errors: &[ErrorDef],
    next_nodes: [&'static CStr; N],
) {
    unsafe {
        let r = &mut *cell.0.get();
        r.reg.function = Some(function);
        r.reg.name = name.as_ptr() as *mut c_char;
        r.reg.type_ = sys::vlib_node_type_t_VLIB_NODE_TYPE_INTERNAL;
        r.reg.vector_size = core::mem::size_of::<u32>() as u8;
        r.reg.format_trace = format_trace;
        if !errors.is_empty() {
            let descs: Vec<sys::vlib_error_desc_t> = errors
                .iter()
                .map(|e| sys::vlib_error_desc_t {
                    name: e.0.as_ptr() as *mut c_char,
                    desc: e.1.as_ptr() as *mut c_char,
                    severity: match e.2 {
                        Severity::Error => sys::vl_counter_severity_e_VL_COUNTER_SEVERITY_ERROR,
                        Severity::Warn => sys::vl_counter_severity_e_VL_COUNTER_SEVERITY_WARN,
                        Severity::Info => sys::vl_counter_severity_e_VL_COUNTER_SEVERITY_INFO,
                    },
                    stats_entry_index: 0,
                })
                .collect();
            r.reg.n_errors = descs.len() as u16;
            r.reg.error_counters = Box::leak(descs.into_boxed_slice()).as_mut_ptr();
        }
        r.reg.n_next_nodes = N as u16;
        for (i, n) in next_nodes.iter().enumerate() {
            r.next_nodes[i] = n.as_ptr();
        }
        // Same linked-list insertion VLIB_REGISTER_NODE's constructor does.
        let vgm = &raw mut sys::vlib_global_main;
        r.reg.next_registration = (*vgm).node_registrations;
        (*vgm).node_registrations = &mut r.reg;
    }
}

/// Buffer indices of the incoming frame.
///
/// # Safety
/// `frame` must be the frame passed to the node function.
pub unsafe fn frame_vector<'a>(frame: *mut sys::vlib_frame_t) -> &'a [u32] {
    unsafe {
        let p = sys::vlib_frame_vector_args(frame) as *const u32;
        core::slice::from_raw_parts(p, (*frame).n_vectors as usize)
    }
}

/// Is packet tracing active for this node right now?
pub fn tracing_enabled(node: *mut sys::vlib_node_runtime_t) -> bool {
    unsafe { (*node).flags & FLAG_TRACE != 0 }
}

/// Streaming enqueue of buffers to next nodes — the Rust equivalent of the
/// `vlib_get_next_frame` / `vlib_validate_buffer_enqueue_x1` /
/// `vlib_put_next_frame` dance. Call `enqueue()` per buffer, `finish()` at
/// the end of the dispatch function.
pub struct NextFrames<'a> {
    vm: *mut sys::vlib_main_t,
    node: *mut sys::vlib_node_runtime_t,
    next_index: u32,
    to_next: *mut u32,
    n_left: u32,
    _lt: PhantomData<&'a ()>,
}

impl<'a> NextFrames<'a> {
    /// # Safety
    /// `vm`/`node` must be the arguments of the running node function.
    pub unsafe fn new(
        vm: *mut sys::vlib_main_t,
        node: *mut sys::vlib_node_runtime_t,
        initial_next: u32,
    ) -> Self {
        let mut s = NextFrames {
            vm,
            node,
            next_index: initial_next,
            to_next: core::ptr::null_mut(),
            n_left: 0,
            _lt: PhantomData,
        };
        unsafe { s.refill() };
        s
    }

    unsafe fn refill(&mut self) {
        unsafe {
            let f = sys::vlib_get_next_frame_internal(self.vm, self.node, self.next_index, 0);
            let start = sys::vlib_frame_vector_args(f) as *mut u32;
            self.to_next = start.add((*f).n_vectors as usize);
            self.n_left = FRAME_SIZE - (*f).n_vectors as u32;
        }
    }

    unsafe fn put(&mut self) {
        unsafe { sys::vlib_put_next_frame(self.vm, self.node, self.next_index, self.n_left) };
    }

    pub fn enqueue(&mut self, buffer_index: u32, next: u32) {
        unsafe {
            if next != self.next_index {
                self.put();
                self.next_index = next;
                self.refill();
            }
            *self.to_next = buffer_index;
            self.to_next = self.to_next.add(1);
            self.n_left -= 1;
            if self.n_left == 0 {
                self.put();
                self.refill();
            }
        }
    }

    pub fn finish(mut self) {
        unsafe { self.put() };
    }
}
