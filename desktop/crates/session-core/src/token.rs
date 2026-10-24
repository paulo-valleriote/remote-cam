use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::RandomSource;

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct PairingToken([u8; 32]);

impl PairingToken {
    pub const LENGTH: usize = 32;

    pub fn generate<R: RandomSource + ?Sized>(random: &mut R) -> Self {
        let mut bytes = [0_u8; Self::LENGTH];
        random.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

impl fmt::Debug for PairingToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PairingToken")
            .field(&"[elided]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use zeroize::{Zeroize, ZeroizeOnDrop};

    use super::PairingToken;
    use crate::RandomSource;
    use crate::testing::SeededRandomSource;

    #[test]
    fn consumes_exactly_the_bytes_the_injected_source_provides() {
        let mut expected = [0_u8; PairingToken::LENGTH];
        SeededRandomSource::new(42).fill_bytes(&mut expected);

        let mut random = SeededRandomSource::new(42);
        let token = PairingToken::generate(&mut random);

        assert_eq!(token.as_bytes(), &expected);
    }

    #[test]
    fn exposes_the_bytes_it_was_built_from() {
        let token = PairingToken::from_bytes([7; PairingToken::LENGTH]);

        assert_eq!(token.as_bytes(), &[7; PairingToken::LENGTH]);
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let rendered = format!("{:?}", PairingToken::from_bytes([7; PairingToken::LENGTH]));

        assert_eq!(rendered, "PairingToken(\"[elided]\")");
        assert!(!rendered.contains("07"));
    }

    #[test]
    fn zeroize_clears_the_buffer() {
        let mut token = PairingToken::from_bytes([0xab; PairingToken::LENGTH]);

        token.zeroize();

        assert_eq!(token.as_bytes(), &[0; PairingToken::LENGTH]);
    }

    #[test]
    fn the_token_wipes_its_buffer_on_drop() {
        fn assert_zeroized_on_drop<T: ZeroizeOnDrop>() {}

        assert_zeroized_on_drop::<PairingToken>();
    }
}
