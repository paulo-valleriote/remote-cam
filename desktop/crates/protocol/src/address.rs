//! Private-address rules shared by the signaling schema and the QR payload.

use std::net::IpAddr;

/// Returns the 16-byte network representation when `value` is a private address
/// permitted by the contract.
///
/// IPv4: RFC 1918 only (`10/8`, `172.16/12`, `192.168/16`). APIPA `169.254/16`
/// is deliberately excluded: it is not reachable across a local network even
/// though the proposed signaling JSON Schema pattern lists it.
/// IPv6: link-local `fe80::/10` and unique-local `fc00::/7`.
pub fn parse_private_address(value: &str) -> Option<[u8; 16]> {
    match value.parse::<IpAddr>().ok()? {
        IpAddr::V4(address) => {
            if is_private_ipv4(address.octets()) {
                let mut packed = [0_u8; 16];
                packed[12..].copy_from_slice(&address.octets());
                Some(packed)
            } else {
                None
            }
        }
        IpAddr::V6(address) => {
            let octets = address.octets();
            if is_private_ipv6(&octets) {
                Some(octets)
            } else {
                None
            }
        }
    }
}

/// True when the string is a private address permitted by the contract.
pub fn is_private_address(value: &str) -> bool {
    parse_private_address(value).is_some()
}

/// True for RFC 1918 IPv4 addresses.
pub fn is_private_ipv4(octets: [u8; 4]) -> bool {
    octets[0] == 10
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 168)
}

/// True for IPv6 link-local (`fe80::/10`) and unique-local (`fc00::/7`) addresses.
pub fn is_private_ipv6(octets: &[u8; 16]) -> bool {
    let link_local = octets[0] == 0xfe && (octets[1] & 0xc0) == 0x80;
    let unique_local = (octets[0] & 0xfe) == 0xfc;
    link_local || unique_local
}

/// Formats a 16-byte packed address, using the low four bytes for IPv4 records.
pub fn format_address(family: u8, packed: &[u8; 16]) -> Option<String> {
    match family {
        4 => {
            if packed[..12].iter().any(|byte| *byte != 0) {
                return None;
            }
            Some(format!(
                "{}.{}.{}.{}",
                packed[12], packed[13], packed[14], packed[15]
            ))
        }
        6 => {
            let mut octets = [0_u8; 16];
            octets.copy_from_slice(packed);
            Some(std::net::Ipv6Addr::from(octets).to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_rfc1918_and_ipv6_private_ranges() {
        for address in [
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.42",
            "fe80::1",
            "fd12:3456::1",
            "fc00::1",
        ] {
            assert!(is_private_address(address), "{address} should be private");
        }
    }

    #[test]
    fn rejects_public_and_special_addresses() {
        for address in [
            "8.8.8.8",
            "172.32.0.1",
            "169.254.1.1",
            "192.169.0.1",
            "2001:4860:4860::8888",
            "not-an-address",
        ] {
            assert!(!is_private_address(address), "{address} must be rejected");
        }
    }

    #[test]
    fn formats_ipv4_and_ipv6_records() {
        let mut packed = [0_u8; 16];
        packed[12..].copy_from_slice(&[192, 168, 1, 42]);
        assert_eq!(format_address(4, &packed).as_deref(), Some("192.168.1.42"));

        let mut not_ipv4 = packed;
        not_ipv4[0] = 1;
        assert_eq!(format_address(4, &not_ipv4), None);

        let mut v6 = [0_u8; 16];
        v6[0] = 0xfe;
        v6[1] = 0x80;
        v6[15] = 1;
        assert_eq!(format_address(6, &v6).as_deref(), Some("fe80::1"));
        assert_eq!(format_address(5, &v6), None);
    }
}
