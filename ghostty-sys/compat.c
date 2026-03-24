/**
 * Weak symbol stubs for ghostty embedded API functions that may not exist
 * in all versions of libghostty. If the real library provides these symbols,
 * the linker uses those. Otherwise, these no-op stubs satisfy the link.
 *
 * This allows cmux-gtk to build against upstream ghostty releases that
 * don't yet expose these functions.
 */

__attribute__((weak))
void ghostty_surface_display_realized(void *surface) {
    (void)surface;
}

__attribute__((weak))
void ghostty_surface_display_unrealized(void *surface) {
    (void)surface;
}
