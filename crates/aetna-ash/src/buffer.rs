use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator};

use crate::runner::{Error, Result};

pub(crate) struct GpuBuffer {
    pub buffer: vk::Buffer,
    pub size: vk::DeviceSize,
    allocation: Option<Allocation>,
}

impl GpuBuffer {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        name: &'static str,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
    ) -> Result<Self> {
        let create_info = vk::BufferCreateInfo::default()
            .size(size.max(1))
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { device.create_buffer(&create_info, None) }?;
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let allocation = allocator.allocate(&AllocationCreateDesc {
            name,
            requirements,
            location,
            linear: true,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        unsafe {
            device.bind_buffer_memory(buffer, allocation.memory(), allocation.offset())?;
        }
        Ok(Self {
            buffer,
            size: size.max(1),
            allocation: Some(allocation),
        })
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() as vk::DeviceSize > self.size {
            return Err(Error::BufferTooSmall {
                requested: bytes.len(),
                capacity: self.size as usize,
            });
        }
        let allocation = self
            .allocation
            .as_mut()
            .ok_or(Error::ResourceDestroyed("buffer allocation"))?;
        let mapped = allocation
            .mapped_slice_mut()
            .ok_or(Error::UnmappedAllocation)?;
        mapped[..bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        if self.buffer != vk::Buffer::null() {
            unsafe {
                device.destroy_buffer(self.buffer, None);
            }
            self.buffer = vk::Buffer::null();
        }
        if let Some(allocation) = self.allocation.take() {
            let _ = allocator.free(allocation);
        }
    }
}
