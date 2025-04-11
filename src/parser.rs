use winnow::ascii::alphanumeric1;
use winnow::combinator::preceded;
use winnow::error::EmptyError;
use winnow::Parser;

pub fn get_blob(input: &mut &str) -> Option<String> {
    preceded("blob", alphanumeric1::<&str, EmptyError>)
        .verify(|s: &str| s.len() > 30)
        .parse_next(input)
        .map(|s| format!("blob{s}"))
        .ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_blob_none() {
        let input = "blab";
        assert_eq!(get_blob(&mut input.clone()), None);

        let input = "blobtooshort";
        assert_eq!(get_blob(&mut input.clone()), None);

        let input = "blob in the middle of a sentence that is very loooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooooong";
        assert_eq!(get_blob(&mut input.clone()), None);
    }

    #[test]
    fn test_get_blob() {
        let input = "blobacpageznmeauqi5wrg45i53tsqbt7336zjcpmrqv4mazl4u5hj6lyajdnb2hi4dthixs65ltmuys2mjoojswyylzf";
        assert_eq!(get_blob(&mut input.clone()), Some(input.to_string()));
    }
}
