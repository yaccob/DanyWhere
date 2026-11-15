//! DanyWhere: tells the local test builds (Dev, NoAX) from the release at a
//! glance (user, 2026-11-15). The build gives each test build its own product
//! name ("DanyWhere Dev", "DanyWhere NoAX"); the release's product name is
//! "DanyWhere", so its window keeps that title.

/// The main window's title for an app with this product name.
pub fn main_window_title(product_name: &str) -> String {
    product_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_window_is_titled_danywhere() {
        assert_eq!(main_window_title("DanyWhere"), "DanyWhere");
    }

    #[test]
    fn the_dev_window_names_its_build() {
        assert_eq!(main_window_title("DanyWhere Dev"), "DanyWhere Dev");
    }

    #[test]
    fn the_noax_window_names_its_build() {
        assert_eq!(main_window_title("DanyWhere NoAX"), "DanyWhere NoAX");
    }
}
