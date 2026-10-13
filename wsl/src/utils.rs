use rand::prelude::*;

use wsl_sys::windows_core::HSTRING;

pub(crate) fn hstring(s: impl AsRef<str>) -> HSTRING {
    HSTRING::from(s.as_ref())
}

pub(crate) fn gen_random_string(length: usize) -> String {
    String::from_iter(
        rand::rng()
            .sample_iter(&rand::distr::Alphanumeric)
            .take(length)
            .map(char::from)
            .collect::<Vec<char>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hstring_converts_str_and_string() {
        assert_eq!(hstring("abc"), HSTRING::from("abc"));
        assert_eq!(String::from_utf16_lossy(&hstring(String::from("ünï"))), "ünï");
    }
}
