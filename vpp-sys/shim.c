/* Tiny C helpers for things Rust cannot do portably: va_list access.
 * VPP format/unformat callbacks receive a va_list* and pull typed
 * arguments out of it; Rust has no stable va_arg, so we extract here. */

#include <stdarg.h>

void *
vppsys_va_arg_ptr (void *ap)
{
  return va_arg (*(va_list *) ap, void *);
}

unsigned int
vppsys_va_arg_u32 (void *ap)
{
  return va_arg (*(va_list *) ap, unsigned int);
}

unsigned long
vppsys_va_arg_uword (void *ap)
{
  return va_arg (*(va_list *) ap, unsigned long);
}
