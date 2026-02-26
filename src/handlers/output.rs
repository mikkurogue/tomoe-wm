//! Output, SHM, and DMA-BUF handlers

use smithay::{
    backend::allocator::dmabuf::Dmabuf,
    wayland::{
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        output::OutputHandler,
        shm::{ShmHandler, ShmState},
    },
};

use crate::state::TomoeState;

// SHM handler
impl ShmHandler for TomoeState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

// DMA-BUF handler
impl DmabufHandler for TomoeState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // For now, accept all dmabufs - in a real compositor you'd validate with the renderer
        self.dmabuf_imported = Some(dmabuf);
        let _ = notifier.successful::<TomoeState>();
    }
}

// Output handler
impl OutputHandler for TomoeState {}
