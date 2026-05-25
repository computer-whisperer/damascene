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

    /// Read this buffer's host-mapped bytes. Only valid for host-visible
    /// allocations (`CpuToGpu` / `GpuToCpu`); the caller must ensure the GPU
    /// has finished writing (e.g. the frame's fence has signalled).
    pub(crate) fn read_bytes(&self) -> Result<&[u8]> {
        self.allocation
            .as_ref()
            .and_then(|a| a.mapped_slice())
            .ok_or(Error::UnmappedAllocation)
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

pub(crate) struct GpuImage {
    pub image: vk::Image,
    pub view: vk::ImageView,
    allocation: Option<gpu_allocator::vulkan::Allocation>,
}

impl GpuImage {
    pub(crate) fn new(
        device: &ash::Device,
        allocator: &mut Allocator,
        name: &'static str,
        format: vk::Format,
        extent: vk::Extent2D,
        usage: vk::ImageUsageFlags,
    ) -> Result<Self> {
        Self::new_attachment(
            device,
            allocator,
            name,
            format,
            extent,
            usage,
            vk::SampleCountFlags::TYPE_1,
            vk::ImageAspectFlags::COLOR,
        )
    }

    /// Like [`GpuImage::new`] but with an explicit sample count and view
    /// aspect — for multisampled colour attachments and depth images that
    /// the scene renderer needs.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_attachment(
        device: &ash::Device,
        allocator: &mut Allocator,
        name: &'static str,
        format: vk::Format,
        extent: vk::Extent2D,
        usage: vk::ImageUsageFlags,
        samples: vk::SampleCountFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<Self> {
        let create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: extent.width.max(1),
                height: extent.height.max(1),
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(samples)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { device.create_image(&create_info, None) }?;
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation = allocator.allocate(&AllocationCreateDesc {
            name,
            requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })?;
        unsafe {
            device.bind_image_memory(image, allocation.memory(), allocation.offset())?;
        }
        let subresource = vk::ImageSubresourceRange::default()
            .aspect_mask(aspect)
            .level_count(1)
            .layer_count(1);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(subresource);
        let view = unsafe { device.create_image_view(&view_info, None) }?;
        Ok(Self {
            image,
            view,
            allocation: Some(allocation),
        })
    }

    pub(crate) unsafe fn destroy(&mut self, device: &ash::Device, allocator: &mut Allocator) {
        if self.view != vk::ImageView::null() {
            unsafe {
                device.destroy_image_view(self.view, None);
            }
            self.view = vk::ImageView::null();
        }
        if self.image != vk::Image::null() {
            unsafe {
                device.destroy_image(self.image, None);
            }
            self.image = vk::Image::null();
        }
        if let Some(allocation) = self.allocation.take() {
            let _ = allocator.free(allocation);
        }
    }
}
