//! Direction and candidate policy for the SDP bodies of `offer` and `answer`.
//!
//! The mobile is the only sender: an offer may only ask the desktop to receive,
//! an answer may only receive, and neither may carry a data channel or a
//! candidate that leaves the local network.

use crate::address::is_private_address;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SdpRole {
    Offer,
    Answer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Sendrecv,
    Sendonly,
    Recvonly,
    Inactive,
}

impl Direction {
    fn parse(attribute: &str) -> Option<Self> {
        match attribute {
            "sendrecv" => Some(Self::Sendrecv),
            "sendonly" => Some(Self::Sendonly),
            "recvonly" => Some(Self::Recvonly),
            "inactive" => Some(Self::Inactive),
            _ => None,
        }
    }
}

impl SdpRole {
    fn permits(self, direction: Option<Direction>) -> bool {
        let direction = direction.unwrap_or(Direction::Sendrecv);
        match self {
            Self::Offer => matches!(direction, Direction::Sendonly | Direction::Inactive),
            Self::Answer => matches!(direction, Direction::Recvonly | Direction::Inactive),
        }
    }
}

#[derive(Default)]
struct MediaSection {
    direction: Option<Direction>,
}

pub(crate) fn is_permitted(text: &str, role: SdpRole) -> bool {
    if !has_only_unambiguous_lines(text) {
        return false;
    }
    let mut session_direction = None;
    let mut section: Option<MediaSection> = None;
    for line in text.lines() {
        if let Some(media) = line.strip_prefix("m=") {
            if let Some(finished) = section.take()
                && !role.permits(finished.direction.or(session_direction))
            {
                return false;
            }
            if !matches!(media.split(' ').next(), Some("audio" | "video")) {
                return false;
            }
            section = Some(MediaSection::default());
        } else if let Some(candidate) = line.strip_prefix("a=candidate:") {
            if !is_local_host_candidate(candidate) {
                return false;
            }
        } else if let Some(direction) = line.strip_prefix("a=").and_then(Direction::parse) {
            let declared = match section.as_mut() {
                Some(current) => &mut current.direction,
                None => &mut session_direction,
            };
            if declared.replace(direction).is_some() {
                return false;
            }
        }
    }
    section.is_none_or(|last| role.permits(last.direction.or(session_direction)))
}

fn has_only_unambiguous_lines(text: &str) -> bool {
    let bare_carriage_return = text.replace("\r\n", "\n").contains('\r');
    !bare_carriage_return
        && !text
            .chars()
            .any(|c| c.is_control() && c != '\r' && c != '\n')
}

fn is_local_host_candidate(candidate: &str) -> bool {
    let fields: Vec<&str> = candidate.split(' ').collect();
    matches!(
        fields.as_slice(),
        [_foundation, _component, transport, _priority, address, _port, "typ", "host", ..]
            if transport.eq_ignore_ascii_case("udp") && is_private_address(address)
    )
}
