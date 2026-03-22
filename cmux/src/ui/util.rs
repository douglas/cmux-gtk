//! GTK utility helpers — iterators, widget lookup, etc.

use gtk4::prelude::*;

/// Iterator over the direct children of a GTK widget.
///
/// # Example
/// ```ignore
/// for child in children(&my_box) {
///     child.set_visible(false);
/// }
/// ```
pub fn children(widget: &impl IsA<gtk4::Widget>) -> ChildIter {
    ChildIter {
        next: widget.as_ref().first_child(),
    }
}

pub struct ChildIter {
    next: Option<gtk4::Widget>,
}

impl Iterator for ChildIter {
    type Item = gtk4::Widget;

    fn next(&mut self) -> Option<gtk4::Widget> {
        let current = self.next.take()?;
        self.next = current.next_sibling();
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    // GTK widget tests require a display, so we just verify the type compiles.
    #[test]
    fn child_iter_is_send() {
        // ChildIter holds Option<gtk4::Widget> which is !Send,
        // so this is expected to fail — just documenting the constraint.
        fn _assert_not_send<T: Send>() {}
        // _assert_not_send::<super::ChildIter>(); // Would fail — GTK widgets are !Send
    }
}
