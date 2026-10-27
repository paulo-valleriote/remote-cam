# RemoteCam Glossary

The vocabulary the project uses in issues, code, tests and reviews. When a term
below is needed, use it exactly as defined here; do not drift to a synonym the
entry rules out.

The terms extend the source glossary in `docs/tdd-remotecam-v1.md` §13 and the
wire vocabulary in `protocol/README.md`, `protocol/qr-payload.md` and
`protocol/reason-codes.json`.

| Term | Definition |
| --- | --- |
| **Session** | An ephemeral, authenticated link between one mobile device and one desktop, opened by scanning a QR and ended without persisted state. At most one session is active on a desktop at any moment (invariant I4). |
| **Session Core** | The product subsystem that owns the signaling contract, the session state machine (`IDLE → PAIRING_READY → HANDSHAKING → NEGOTIATING → CONNECTED → ENDED`; `SUPERSEDED` is the predecessor's retired state, not a resting state of the core), epoch handling, pairing validation and the API a harness drives. `CONNECTED` requires an accepted `answer` and the first **Media Frame**; a first-frame timeout returns the current attempt to `IDLE` rather than ending a session. |
| **Media Port** | The boundary through which the Session Core starts a media source and receives frame observations. |
| **Media Frame** | A numbered media sample with the instant at which it was produced. |
| **Media Core** | The receive-only subsystem that turns the RTP media of an active **Session** into **Media Frames** and continuous audio, behind the **Media Port**. It never originates media. |
| **Frame Ring** | The bounded buffer of decoded video frames between the **Media Core** and the **Virtual Device** producer. When the consumer lags it drops the oldest frame; it never blocks the network thread. |
| **Receive-only Invariant** | The desktop never originates media RTP, in any state of a **Session**. Control RTCP is allowed. A non-zero count of outbound media RTP is a failure, not a metric. |
| **Virtual Device** | The camera or microphone the desktop publishes so that other applications consume the phone's media. The virtual camera is user-mode; the virtual microphone is a third-party audio device the app detects and verifies but does not redistribute. |
| **A/V Drift** | The offset between the audio and the video presented for the same instant, with audio as the master clock. The target is 20 ms over one hour; 100 ms is the hard limit and ends the **Session** with `media_failure`. |
| **Pairing** | The act of establishing trust between a mobile device and a desktop: scan the desktop-displayed QR payload, complete a TLS 1.3 handshake against the exact pinned certificate, then authenticate with the single-use pairing token. Pairing is the only way a session starts. |
| **Pairing Token** | The 256-bit single-use secret carried in the QR payload. It is generated per session, compared in constant time, consumed when it matches on the first authenticated frame (a mismatching presentation does not consume it), and a presentation of an already consumed token is refused for the lifetime of the listener: with `superseded` while a session is active (`NEGOTIATING` or `CONNECTED`, the active session is unchanged), and with `token_already_used` when no session is active (`IDLE`, `PAIRING_READY`, `HANDSHAKING`, `ENDED`). Any other first frame while a session is active and no QR is pending (a token never issued, a frame that is not `pair`) is refused with `protocol_violation`, and the active session is unchanged. The token is never persisted, logged or copied to the clipboard. |
| **Pairing Window** | The 120-second validity period of a QR payload and its Pairing Token, starting when the QR is created. At or after its end, the material is refused with `qr_expired` (before the handshake) or `token_expired` (at first-frame authentication). |
| **Pending QR** | The QR offered by the desktop and not yet consumed: created by starting pairing, valid for one **Pairing Window**. While a session is active, a peer that connects with no pending QR is refused as `superseded`. A pending QR alongside an active session is what makes a new scan a **Supersession**. See ADR-0013. |
| **Epoch** | The monotonic 32-bit session generation assigned by the desktop when the QR is created. It is strictly greater than the epoch of the active session; a message carrying a lower epoch is discarded silently and counted as a protocol violation. It restarts at 1 when the desktop process restarts. See ADR-0004. |
| **Supersession** | The replacement of the active session by a newer valid pairing. The desktop sends `close { superseded }` to the previous device and destroys its state before the new pairing completes; media overlap is forbidden. The predecessor ends in the retired state `SUPERSEDED`; the Session Core itself then reports the successor's state. See ADR-0005 and ADR-0013. |
| **Harness** | A product-owned executable test driver that exercises the Session Core end to end through its real interfaces, with an injected clock and RNG, and emits a deterministic machine-readable report. A harness is not shipped to users and is not a spike. See ADR-0007. |
| **Spike Harness** | A disposable F0 measurement driver kept under `spikes/`. It produces evidence for a risk-reduction gate, is exempt from the coverage gate, and MUST NOT be merged into `desktop/` or `mobile/` (FR-039). |
| **Quarantine** | The state of F0 spike code that has been moved out of the product trees into `spikes/` and retained only for provenance and reproducibility. Quarantined code is non-production, is not wired into any product build, and cannot be promoted without a new implementation task. See ADR-0003. |
| **Race Loser** | The peer that presents a **Pairing Token** already consumed by the peer that won the race for the same QR, or that connects after the race was decided. It is refused with `superseded`: as `close { superseded }` when it already has a channel, and as a socket closed without a frame, with the reason kept local, when it has not completed the handshake. The active session is unchanged. See ADR-0011 and ADR-0013. |
| **Reference Device** | The pinned configuration a gate measurement is valid on: a Windows 11 desktop with a recent 4-core CPU and integrated GPU, a mid-range Android phone from 2021 or newer, and a Wi-Fi 5 router at 5 GHz within 5 m. Measurements taken outside it must be flagged as such. |
| **F1 Core Exit** | The F1 exit gate over the protocol core, session core and product harness (deadline 2026-11-20). It has no device dependency; the device residue inspection is tracked separately as **C-02-residual**. See ADR-0017. |
| **C-02-residual** | The residue inspection after application restart, on two named reference devices, with the same method as the session-end inspection (deadline 2026-12-20). It continues the F0 condition `C-02-residue-restart` and does not gate the **F1 Core Exit**. See ADR-0017. |
| **F3 Core Exit** | The F3 exit gate over the **Media Core**, proven on the Linux harness with an injected clock and emulated network (deadline 2026-12-18). It has no device dependency. See ADR-0019. |
| **F3 Device Exit** | The F3 exit gate over the **Virtual Devices** on the Windows 11 **Reference Device**: the minimum application matrix, glass-to-glass latency and the virtual audio path (deadline 2027-01-15). F4 depends on it. See ADR-0019. |
| **Last-pairing-wins** | The rule that the most recent valid pairing owns the desktop. It is enforced by supersession plus the monotonic epoch, and it delivers invariant I4: at most one active session, with no media overlap. |

## Terms deliberately avoided

| Avoid | Use instead | Why |
| --- | --- | --- |
| "connection", "link" | **Session** | "Connection" names the transport; the domain concept is the authenticated session. |
| "pairing code", "PIN" | **Pairing Token** | The secret is a 256-bit token in the QR payload, not a short user-typed code. |
| "reconnect", "resume" | **Pairing** (again) | Session state is never persisted; a new link is always a new pairing. |
| "virtual camera driver", "kernel driver" (for the camera) | **Virtual Device** | The virtual camera is a user-mode media source; no kernel driver is involved. "Driver" applies only to the third-party audio device. |
| "test suite" (for a gate driver) | **Spike Harness** or **Harness** | The two have different ownership, location and quality rules. |
