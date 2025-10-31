use crate::vr::{GpuFrameSubmission, TrackedPose, VrError, VrInputSample};
use crate::render::WgpuContext;
use openxr::{self as xr, HandleWrapper, Path, Session, Space, Swapchain, SwapchainCreateFlags};
use std::sync::Arc;

pub struct XrSession {
    session: Session<xr::Vulkan>
}
