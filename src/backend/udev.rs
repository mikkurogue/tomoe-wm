//! Udev/DRM backend for running as a native compositor from TTY
//!
//! This backend uses DRM (Direct Rendering Manager) for display output and
//! libinput for input handling. It allows running Tomoe as a standalone
//! compositor directly from a Linux TTY without needing X11 or Wayland.

use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
            DrmDevice, DrmDeviceFd, DrmEvent, DrmEventMetadata, DrmNode, NodeType,
        },
        egl::EGLDisplay,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            damage::OutputDamageTracker,
            element::{
                surface::WaylandSurfaceRenderElement, AsRenderElements, RenderElement,
                RenderElementStates,
            },
            gles::GlesRenderer,
            ImportDma, Renderer,
        },
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::{self, UdevBackend, UdevEvent},
    },
    desktop::layer_map_for_output,
    output::{Mode, Output, PhysicalProperties},
    reexports::{
        calloop::{EventLoop, RegistrationToken},
        drm::control::{connector, crtc, ModeTypeFlags},
        input::Libinput,
        rustix::fs::OFlags,
    },
    utils::{DeviceFd, Point, Scale, Size, Transform},
    wayland::shell::wlr_layer::Layer as WlrLayer,
};
use smithay_drm_extras::drm_scanner::{DrmScanEvent, DrmScanner, SimpleCrtcMapper};

use std::{collections::HashMap, path::Path, time::Duration};
use tracing::{debug, error, info, warn};

use crate::state::TomoeState;

/// Supported color formats for DRM
const SUPPORTED_COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Xrgb8888];

/// State for the udev backend - this must persist for the lifetime of the compositor
pub struct UdevData {
    pub session: LibSeatSession,
    pub primary_gpu: DrmNode,
    pub devices: HashMap<DrmNode, OutputDevice>,
    /// Whether the session is currently active (not switched to another VT)
    pub session_active: bool,
}

/// Data associated with a DRM device (GPU)
pub struct OutputDevice {
    pub node: DrmNode,
    pub drm: DrmDevice,
    pub gbm: GbmDevice<DrmDeviceFd>,
    pub gles: GlesRenderer,
    pub drm_scanner: DrmScanner<SimpleCrtcMapper>,
    pub surfaces: HashMap<crtc::Handle, Surface>,
    pub registration_token: RegistrationToken,
}

/// Surface for rendering to a specific output (monitor)
pub struct Surface {
    pub output: Output,
    pub crtc: crtc::Handle,
    pub connector: connector::Handle,
    pub compositor: GbmDrmCompositor,
    pub damage_tracker: OutputDamageTracker,
}

/// Type alias for the DRM compositor we use
type GbmDrmCompositor = DrmCompositor<
    GbmAllocator<DrmDeviceFd>,
    GbmFramebufferExporter<DrmDeviceFd>,
    (), // User data for presentation feedback
    DrmDeviceFd,
>;

/// Initialize the udev/DRM backend
pub fn init_udev(
    event_loop: &mut EventLoop<TomoeState>,
    state: &mut TomoeState,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize session (libseat handles seat access without root)
    let (session, notifier) = LibSeatSession::new()?;
    info!("Session initialized on seat: {}", session.seat());

    // Find the primary GPU
    let primary_gpu = udev::primary_gpu(&session.seat())
        .ok()
        .flatten()
        .and_then(|path| DrmNode::from_path(&path).ok())
        .ok_or("Failed to find primary GPU")?;
    info!("Primary GPU: {:?}", primary_gpu);

    // Create udev data that will be stored in state
    let udev_data = UdevData {
        session: session.clone(),
        primary_gpu,
        devices: HashMap::new(),
        session_active: true,
    };
    state.udev_data = Some(udev_data);

    // Insert session event source
    event_loop
        .handle()
        .insert_source(notifier, |event, _, state| {
            handle_session_event(state, event);
        })?;

    // Initialize udev backend for GPU hotplug
    let udev_backend = UdevBackend::new(&session.seat())?;

    // Process existing GPUs
    for (device_id, path) in udev_backend.device_list() {
        info!("Found GPU device {:?} at {:?}", device_id, path);
        if let Err(e) = device_added(event_loop, state, &path) {
            warn!("Failed to initialize GPU {:?}: {}", device_id, e);
        }
    }

    // Insert udev event source for hotplug
    event_loop
        .handle()
        .insert_source(udev_backend, move |event, _, state| {
            handle_udev_event(state, event);
        })?;

    // Initialize libinput for input handling
    let mut libinput_context =
        Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));

    if let Err(e) = libinput_context.udev_assign_seat(&session.seat()) {
        return Err(format!("Failed to assign seat to libinput: {:?}", e).into());
    }

    let libinput_backend = LibinputInputBackend::new(libinput_context.clone());

    event_loop
        .handle()
        .insert_source(libinput_backend, |event, _, state| {
            crate::input::handle_libinput_event(state, event);
        })?;

    info!("Udev backend initialized successfully");
    Ok(())
}

/// Handle a new GPU device being added
fn device_added(
    event_loop: &mut EventLoop<TomoeState>,
    state: &mut TomoeState,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let node = DrmNode::from_path(path)?;

    // Only handle primary nodes
    if node.ty() != NodeType::Primary {
        debug!("Skipping non-primary node: {:?}", node);
        return Ok(());
    }

    let udev_data = state
        .udev_data
        .as_mut()
        .ok_or("Udev data not initialized")?;

    // Skip if already initialized
    if udev_data.devices.contains_key(&node) {
        debug!("Device already initialized: {:?}", node);
        return Ok(());
    }

    info!("Initializing GPU: {:?}", node);

    // Open the DRM device through the session
    let fd = udev_data.session.open(
        path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;

    let drm_fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(drm_fd.clone(), true)?;

    // Create GBM device for buffer allocation
    let gbm = GbmDevice::new(drm_fd.clone())?;

    // Create EGL display and GLES renderer
    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let egl_context = smithay::backend::egl::EGLContext::new(&egl_display)?;
    let gles = unsafe { GlesRenderer::new(egl_context)? };

    // Initialize dmabuf support for the primary GPU
    if node == udev_data.primary_gpu && state.dmabuf_global.is_none() {
        let dmabuf_formats = gles.dmabuf_formats();
        let dmabuf_global = state
            .dmabuf_state
            .create_global::<TomoeState>(&state.display_handle, dmabuf_formats);
        state.dmabuf_global = Some(dmabuf_global);
        info!("DMA-BUF initialized for primary GPU");
    }

    // Insert DRM event source for VBlank handling
    let node_copy = node;
    let token =
        event_loop
            .handle()
            .insert_source(drm_notifier, move |event, metadata, state| {
                handle_drm_event(state, node_copy, event, metadata);
            })?;

    // Create the output device
    let mut device = OutputDevice {
        node,
        drm,
        gbm,
        gles,
        drm_scanner: DrmScanner::new(),
        surfaces: HashMap::new(),
        registration_token: token,
    };

    // Scan for connected connectors and set up outputs
    scan_connectors(state, &mut device)?;

    // Store the device
    let udev_data = state.udev_data.as_mut().unwrap();
    udev_data.devices.insert(node, device);

    info!("GPU {:?} initialized successfully", node);
    Ok(())
}

/// Scan connectors on a device and set up outputs for connected ones
fn scan_connectors(
    state: &mut TomoeState,
    device: &mut OutputDevice,
) -> Result<(), Box<dyn std::error::Error>> {
    let scan_result = device.drm_scanner.scan_connectors(&device.drm)?;

    for event in scan_result {
        match event {
            DrmScanEvent::Connected { connector, crtc } => {
                if let Some(crtc) = crtc {
                    let connector_handle = connector.handle();
                    if let Err(e) = connector_connected(state, device, connector, crtc) {
                        warn!("Failed to set up connector {:?}: {}", connector_handle, e);
                    }
                }
            }
            DrmScanEvent::Disconnected { connector, crtc } => {
                if let Some(crtc) = crtc {
                    connector_disconnected(state, device, connector.handle(), crtc);
                }
            }
        }
    }

    Ok(())
}

/// Handle a connector being connected - creates a Surface with DrmCompositor
fn connector_connected(
    state: &mut TomoeState,
    device: &mut OutputDevice,
    connector: connector::Info,
    crtc: crtc::Handle,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get connector name
    let connector_name = format!(
        "{}-{}",
        connector.interface().as_str(),
        connector.interface_id()
    );
    info!("Connector connected: {} on CRTC {:?}", connector_name, crtc);

    // Find the best mode (prefer the one marked as preferred, or the first one)
    let mode = connector
        .modes()
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| connector.modes().first())
        .copied()
        .ok_or_else(|| format!("No modes available for connector {}", connector_name))?;

    let (width, height) = mode.size();
    let refresh = mode.vrefresh() as i32 * 1000;

    info!(
        "Selected mode: {}x{}@{}Hz for {}",
        width,
        height,
        refresh / 1000,
        connector_name
    );

    // Create the DRM surface
    let drm_surface = device
        .drm
        .create_surface(crtc, mode, &[connector.handle()])?;

    // Create the GBM allocator
    let gbm_flags = GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT;
    let allocator = GbmAllocator::new(device.gbm.clone(), gbm_flags);

    // Get render formats from the renderer
    let render_formats = device.gles.dmabuf_formats().clone();

    // Create the Smithay output first (needed for OutputModeSource)
    let (physical_width, physical_height) = connector.size().unwrap_or((0, 0));

    let output = Output::new(
        connector_name.clone(),
        PhysicalProperties {
            size: (physical_width as i32, physical_height as i32).into(),
            subpixel: connector.subpixel().into(),
            make: "Unknown".into(),
            model: "Unknown".into(),
        },
    );

    let output_mode = Mode {
        size: (width as i32, height as i32).into(),
        refresh,
    };

    let position = calculate_output_position(state, &connector_name);
    output.change_current_state(
        Some(output_mode),
        Some(Transform::Normal),
        None,
        Some(position),
    );
    output.set_preferred(output_mode);

    // Create the GBM framebuffer exporter
    let fb_exporter = GbmFramebufferExporter::new(device.gbm.clone(), None);

    // Create the DRM compositor
    let compositor = DrmCompositor::new(
        smithay::output::OutputModeSource::Auto(output.clone()),
        drm_surface,
        None, // No planes
        allocator,
        fb_exporter,
        SUPPORTED_COLOR_FORMATS,
        render_formats,
        device.drm.cursor_size(),
        Some(device.gbm.clone()),
    )?;

    // Create output global for clients
    output.create_global::<TomoeState>(&state.display_handle);
    state.space.map_output(&output, position);

    // Add output to the layout system (creates a monitor with one workspace)
    state.layout.add_output(output.clone());

    // Update tiling layout for the first output (legacy, for winit compatibility)
    if state.space.outputs().count() == 1 {
        state
            .tiling
            .set_output_size(Size::from((output_mode.size.w, output_mode.size.h)));
    }

    let damage_tracker = OutputDamageTracker::from_output(&output);

    // Create the surface
    let surface = Surface {
        output: output.clone(),
        crtc,
        connector: connector.handle(),
        compositor,
        damage_tracker,
    };

    device.surfaces.insert(crtc, surface);

    info!(
        "Output {} added at ({}, {})",
        connector_name, position.x, position.y
    );

    // Queue initial render
    queue_redraw(state, device.node, crtc);

    Ok(())
}

/// Handle a connector being disconnected
fn connector_disconnected(
    state: &mut TomoeState,
    device: &mut OutputDevice,
    connector: connector::Handle,
    crtc: crtc::Handle,
) {
    info!("Connector disconnected: {:?} on CRTC {:?}", connector, crtc);

    if let Some(surface) = device.surfaces.remove(&crtc) {
        // Remove from layout (windows get orphaned and need to be moved)
        let orphaned_windows = state.layout.remove_output(&surface.output);

        // Move orphaned windows to primary monitor if available
        for window in orphaned_windows {
            state.layout.add_window(window);
        }

        state.space.unmap_output(&surface.output);
        info!("Output {} removed", surface.output.name());
    }
}

/// Calculate position for an output based on config or auto-layout
fn calculate_output_position(
    state: &TomoeState,
    name: &str,
) -> Point<i32, smithay::utils::Logical> {
    // Check if there's a config entry for this output
    if let Some(output_config) = state.config.outputs.iter().find(|o| o.name == name) {
        if let (Some(x), Some(y)) = (output_config.x, output_config.y) {
            return (x, y).into();
        }
    }

    // Auto-layout: place to the right of existing outputs
    let mut max_x = 0i32;
    for output in state.space.outputs() {
        if let Some(geo) = state.space.output_geometry(output) {
            max_x = max_x.max(geo.loc.x + geo.size.w);
        }
    }

    (max_x, 0).into()
}

/// Handle session events (VT switching, etc.)
fn handle_session_event(state: &mut TomoeState, event: SessionEvent) {
    let Some(udev_data) = state.udev_data.as_mut() else {
        return;
    };

    match event {
        SessionEvent::PauseSession => {
            info!("Session paused (VT switch away)");
            udev_data.session_active = false;

            // Pause all DRM devices
            for device in udev_data.devices.values_mut() {
                device.drm.pause();
            }
        }
        SessionEvent::ActivateSession => {
            info!("Session activated (VT switch back)");
            udev_data.session_active = true;

            // Resume all DRM devices and trigger re-render
            for device in udev_data.devices.values_mut() {
                if let Err(e) = device.drm.activate(true) {
                    error!("Failed to activate DRM device: {}", e);
                }

                // Re-scan connectors in case something changed while we were away
                if let Ok(scan_result) = device.drm_scanner.scan_connectors(&device.drm) {
                    for event in scan_result {
                        match event {
                            DrmScanEvent::Connected { connector, crtc } => {
                                if let Some(_crtc) = crtc {
                                    info!(
                                        "Connector re-connected after VT switch: {:?}",
                                        connector.handle()
                                    );
                                    // Note: We'd need to call connector_connected here but we don't
                                    // have mutable state access
                                }
                            }
                            DrmScanEvent::Disconnected { connector, crtc } => {
                                if let Some(_crtc) = crtc {
                                    info!(
                                        "Connector disconnected during VT switch: {:?}",
                                        connector.handle()
                                    );
                                }
                            }
                        }
                    }
                }

                // Queue redraw for all surfaces
                for (_crtc, surface) in &mut device.surfaces {
                    // Reset compositor state after VT switch
                    if let Err(e) = surface.compositor.reset_state() {
                        warn!("Failed to reset compositor state: {}", e);
                    }
                }
            }

            // Collect surfaces to redraw (to avoid borrow conflicts)
            let surfaces_to_redraw: Vec<(DrmNode, crtc::Handle)> = udev_data
                .devices
                .iter()
                .flat_map(|(node, device)| device.surfaces.keys().map(move |crtc| (*node, *crtc)))
                .collect();

            // Queue redraw for all outputs
            for (node, crtc) in surfaces_to_redraw {
                queue_redraw(state, node, crtc);
            }
        }
    }
}

/// Handle udev device events (GPU hotplug)
fn handle_udev_event(_state: &mut TomoeState, event: UdevEvent) {
    match event {
        UdevEvent::Added { device_id, path } => {
            info!("GPU added: {:?} at {:?}", device_id, path);
            // TODO: Properly initialize new GPU (need event loop handle)
        }
        UdevEvent::Changed { device_id } => {
            debug!("GPU changed: {:?}", device_id);
            // Could rescan connectors here
        }
        UdevEvent::Removed { device_id } => {
            info!("GPU removed: {:?}", device_id);
            // TODO: Clean up removed GPU
        }
    }
}

/// Handle DRM events (vblank, page flip)
fn handle_drm_event(
    state: &mut TomoeState,
    node: DrmNode,
    event: DrmEvent,
    metadata: &mut Option<DrmEventMetadata>,
) {
    match event {
        DrmEvent::VBlank(crtc) => {
            on_vblank(state, node, crtc, metadata.take());
        }
        DrmEvent::Error(err) => {
            error!("DRM error on {:?}: {}", node, err);
        }
    }
}

/// Handle VBlank event - this is where we submit the rendered frame and queue the next one
fn on_vblank(
    state: &mut TomoeState,
    node: DrmNode,
    crtc: crtc::Handle,
    _metadata: Option<DrmEventMetadata>,
) {
    let Some(udev_data) = state.udev_data.as_mut() else {
        return;
    };

    if !udev_data.session_active {
        return;
    }

    let Some(device) = udev_data.devices.get_mut(&node) else {
        error!("Missing device in VBlank callback for CRTC {:?}", crtc);
        return;
    };

    let Some(surface) = device.surfaces.get_mut(&crtc) else {
        error!("Missing surface in VBlank callback for CRTC {:?}", crtc);
        return;
    };

    // Mark the last frame as submitted
    match surface.compositor.frame_submitted() {
        Ok(_) => {}
        Err(e) => {
            warn!("Error in frame_submitted: {}", e);
        }
    }

    // Render the next frame
    render_surface(state, node, crtc);
}

/// Queue a redraw for a specific surface
fn queue_redraw(state: &mut TomoeState, node: DrmNode, crtc: crtc::Handle) {
    // For DRM backend, we render on VBlank, but we can trigger an immediate render
    // by rendering now (which will queue and wait for VBlank)
    render_surface(state, node, crtc);
}

/// Render a frame on a specific surface
fn render_surface(state: &mut TomoeState, node: DrmNode, crtc: crtc::Handle) {
    let start_time = state.start_time;

    let Some(udev_data) = state.udev_data.as_mut() else {
        return;
    };

    if !udev_data.session_active {
        return;
    }

    let Some(device) = udev_data.devices.get_mut(&node) else {
        return;
    };

    let Some(surface) = device.surfaces.get_mut(&crtc) else {
        return;
    };

    let output = surface.output.clone();
    let output_scale = Scale::from(output.current_scale().fractional_scale());

    // Collect all render elements in proper stacking order
    let mut render_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();

    // 1. Overlay layer (topmost)
    let layer_map = layer_map_for_output(&output);
    for layer_surface in layer_map.layers_on(WlrLayer::Overlay) {
        if let Some(geo) = layer_map.layer_geometry(layer_surface) {
            let loc = geo.loc.to_physical_precise_round(output_scale);
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                layer_surface.render_elements(&mut device.gles, loc, output_scale, 1.0);
            render_elements.extend(elements);
        }
    }

    // 2. Top layer
    for layer_surface in layer_map.layers_on(WlrLayer::Top) {
        if let Some(geo) = layer_map.layer_geometry(layer_surface) {
            let loc = geo.loc.to_physical_precise_round(output_scale);
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                layer_surface.render_elements(&mut device.gles, loc, output_scale, 1.0);
            render_elements.extend(elements);
        }
    }
    drop(layer_map);

    // 3. Windows from the layout (between Top and Bottom layers)
    let window_elements =
        state
            .layout
            .render_elements_for_output(&output, &mut device.gles, output_scale);
    render_elements.extend(window_elements);

    // 4. Bottom layer
    let layer_map = layer_map_for_output(&output);
    for layer_surface in layer_map.layers_on(WlrLayer::Bottom) {
        if let Some(geo) = layer_map.layer_geometry(layer_surface) {
            let loc = geo.loc.to_physical_precise_round(output_scale);
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                layer_surface.render_elements(&mut device.gles, loc, output_scale, 1.0);
            render_elements.extend(elements);
        }
    }

    // 5. Background layer (bottommost)
    for layer_surface in layer_map.layers_on(WlrLayer::Background) {
        if let Some(geo) = layer_map.layer_geometry(layer_surface) {
            let loc = geo.loc.to_physical_precise_round(output_scale);
            let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                layer_surface.render_elements(&mut device.gles, loc, output_scale, 1.0);
            render_elements.extend(elements);
        }
    }
    drop(layer_map);

    // Render frame using DRM compositor
    let render_result = surface.compositor.render_frame(
        &mut device.gles,
        &render_elements,
        [0.1, 0.1, 0.1, 1.0], // Background color
        FrameFlags::empty(),
    );

    match render_result {
        Ok(render_output) => {
            // Queue the frame for display
            if !render_output.is_empty {
                match surface.compositor.queue_frame(()) {
                    Ok(_) => {
                        debug!("Frame queued for output {}", output.name());
                    }
                    Err(e) => {
                        warn!("Error queueing frame: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Error rendering frame: {}", e);
        }
    }

    // Send frame callbacks to windows on the active workspace
    if let Some(monitor) = state.layout.monitor_for_output(&output) {
        for window in monitor.active_workspace().windows() {
            window.send_frame(
                &output,
                start_time.elapsed(),
                Some(Duration::ZERO),
                |_, _| Some(output.clone()),
            );
        }
    }

    // Send frame callbacks to layer surfaces
    let layer_map = layer_map_for_output(&output);
    for layer_surface in layer_map.layers() {
        layer_surface.send_frame(
            &output,
            start_time.elapsed(),
            Some(Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }

    state.space.refresh();
    state.popups.cleanup();

    // Flush client events
    let _ = state.display_handle.flush_clients();
}
