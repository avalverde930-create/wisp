# client/

The single desktop dogfood client for the MVP. **winit + wgpu** single window — decode + render the host's frames and capture local input — linking `core` directly. NO Tauri, NO webview, NO TS frontend, NO IPC seam (all deferred to Phase 3's polished UI). This is the day-one dogfood per the resolved Open Question #1 (desktop has no app-store gatekeeper, full capability, no WebKit limitation).

## Why winit+wgpu, not Tauri, in the MVP
The Tauri + custom wgpu/D3D video surface + TS frontend is the single most complex build arrangement in the north-star tree and retires ZERO Phase-1 risk. A raw winit+wgpu blit proves the capture->encode->transport->decode->render->inject pipeline. Tauri arrives in Phase 3 when a polished, multi-platform UI is the goal (`apps/desktop/src-tauri`).

## Dependency rules
Links `core` directly (desktop/host is the only FFI-free surface). Never reaches into host/* or services/*.
