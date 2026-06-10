//! core::media — codec negotiation (H.264 low-latency High profile baseline + AV1 tier in
//! Phase 4; HEVC excluded) and the capture->encode->packetize / depacketize->decode->render
//! pipeline orchestration. Concrete WgcSource/NvencEncoder/SoftwareEncoder/Win32SendInput
//! live host-side. The four traits are EXTRACTED from working code, not written ahead of it.
