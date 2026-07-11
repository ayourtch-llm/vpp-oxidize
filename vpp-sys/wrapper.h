/* Input header for bindgen. Pulls in the parts of VPP that the safe
 * wrapper crate builds on. Must be parseable standalone with just
 * -I<vpp-install>/include. */

#include <vlib/vlib.h>
#include <vlib/unix/plugin.h>
#include <vnet/vnet.h>
#include <vnet/plugin/plugin.h>
#include <vnet/feature/feature.h>
#include <vnet/ethernet/ethernet.h>
#include <vnet/ip/ip.h>
#include <vlibapi/api.h>
#include <vlibmemory/api.h>
