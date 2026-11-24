// port of Homebrew's Formulary.class_s
pub fn formula_class(name: &str) -> String {
    let mut chars = name.chars();
    let capitalized: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    };
    let camel = regex::Regex::new(r"[-_.\s]([a-zA-Z0-9])").unwrap().replace_all(&capitalized, |c: &regex::Captures| c[1].to_uppercase());
    let plus = camel.replace('+', "x");
    regex::Regex::new(r"(.)@(\d)").unwrap().replace(&plus, "${1}AT${2}").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names_match_homebrew() {
        assert_eq!(formula_class("ophello"), "Ophello");
        assert_eq!(formula_class("my-app"), "MyApp");
        assert_eq!(formula_class("foo_bar.baz"), "FooBarBaz");
        assert_eq!(formula_class("foo@1"), "FooAT1");
        assert_eq!(formula_class("libc++"), "Libcxx");
        assert_eq!(formula_class("MPZ"), "Mpz");
    }
}
