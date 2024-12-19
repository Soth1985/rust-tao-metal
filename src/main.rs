use core::{cell::OnceCell, ptr::NonNull};

use objc2::{
    declare_class, msg_send_id, mutability::MainThreadOnly, rc::Retained, runtime::ProtocolObject,
    ClassType, DeclaredClass,
};
use objc2_app_kit::{NSWindow};
use objc2_foundation::{ns_string, MainThreadMarker, NSObject, NSObjectProtocol, NSSize};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice,
    MTLLibrary, MTLPackedFloat3, MTLPrimitiveType, MTLRenderCommandEncoder,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState,
};
use objc2_metal_kit::{MTKView, MTKViewDelegate};

use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
    window::Window
};

use tao::platform::macos::WindowExtMacOS;

#[derive(Copy, Clone)]
#[repr(C)]
struct SceneProperties {
    time: f32,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct VertexInput {
    position: MTLPackedFloat3,
    color: MTLPackedFloat3,
}

struct AppState {
    command_queue: OnceCell<Retained<ProtocolObject<dyn MTLCommandQueue>>>,
    pipeline_state: OnceCell<Retained<ProtocolObject<dyn MTLRenderPipelineState>>>,
    window: OnceCell<Retained<NSWindow>>,
    mtk_view: OnceCell<Retained<MTKView>>,
}

// declare the Objective-C class machinery
declare_class!(
    struct MtkViewDelegate;

    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - Main thread only mutability is correct, since this is an application delegate.
    // - `Delegate` does not implement `Drop`.
    unsafe impl ClassType for MtkViewDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "MtkViewDelegate";
    }

    impl DeclaredClass for MtkViewDelegate {
        type Ivars = AppState;
    }

    unsafe impl NSObjectProtocol for MtkViewDelegate {}

    // define the delegate methods for the `MTKViewDelegate` protocol
    unsafe impl MTKViewDelegate for MtkViewDelegate {
        #[method(drawInMTKView:)]
        #[allow(non_snake_case)]
        unsafe fn drawInMTKView(&self, mtk_view: &MTKView) {
            let command_queue = self.ivars().command_queue.get().unwrap();
            let pipeline_state = self.ivars().pipeline_state.get().unwrap();

            // prepare for drawing
            let Some(current_drawable) = (unsafe { mtk_view.currentDrawable() }) else {
                return;
            };
            let Some(command_buffer) = command_queue.commandBuffer() else {
                return;
            };
            let Some(pass_descriptor) = (unsafe { mtk_view.currentRenderPassDescriptor() }) else {
                return;
            };
            let Some(encoder) = command_buffer.renderCommandEncoderWithDescriptor(&pass_descriptor)
            else {
                return;
            };

            // compute the scene properties
            /*let scene_properties_data = &SceneProperties {
                time: unsafe { self.ivars().start_date.timeIntervalSinceNow() } as f32,
            };
            // write the scene properties to the vertex shader argument buffer at index 0
            let scene_properties_bytes = NonNull::from(scene_properties_data);
            unsafe {
                encoder.setVertexBytes_length_atIndex(
                    scene_properties_bytes.cast::<core::ffi::c_void>(),
                    core::mem::size_of_val(scene_properties_data),
                    0,
                )
            };*/

            // compute the triangle geometry
            let vertex_input_data: &[VertexInput] = &[
                VertexInput {
                    position: MTLPackedFloat3 {
                        x: -f32::sqrt(3.0) / 4.0,
                        y: -0.25,
                        z: 0.,
                    },
                    color: MTLPackedFloat3 {
                        x: 1.,
                        y: 0.,
                        z: 0.,
                    },
                },
                VertexInput {
                    position: MTLPackedFloat3 {
                        x: f32::sqrt(3.0) / 4.0,
                        y: -0.25,
                        z: 0.,
                    },
                    color: MTLPackedFloat3 {
                        x: 0.,
                        y: 1.,
                        z: 0.,
                    },
                },
                VertexInput {
                    position: MTLPackedFloat3 {
                        x: 0.,
                        y: 0.5,
                        z: 0.,
                    },
                    color: MTLPackedFloat3 {
                        x: 0.,
                        y: 0.,
                        z: 1.,
                    },
                },
            ];
            // write the triangle geometry to the vertex shader argument buffer at index 1
            let vertex_input_bytes = NonNull::from(vertex_input_data);
            unsafe {
                encoder.setVertexBytes_length_atIndex(
                    vertex_input_bytes.cast::<core::ffi::c_void>(),
                    core::mem::size_of_val(vertex_input_data),
                    1,
                )
            };

            // configure the encoder with the pipeline and draw the triangle
            encoder.setRenderPipelineState(pipeline_state);
            unsafe {
                encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3)
            };
            encoder.endEncoding();

            // schedule the command buffer for display and commit
            command_buffer.presentDrawable(ProtocolObject::from_ref(&*current_drawable));
            command_buffer.commit();
        }

        #[method(mtkView:drawableSizeWillChange:)]
        #[allow(non_snake_case)]
        unsafe fn mtkView_drawableSizeWillChange(&self, _view: &MTKView, _size: NSSize) {
            //println!("mtkView_drawableSizeWillChange");
        }
    }
);

impl MtkViewDelegate {
    fn init(&self) {
        let mtm = MainThreadMarker::new().unwrap();
        let window = self.ivars().window.get().unwrap();
        // get the default device
        let device = {
            let ptr = unsafe { MTLCreateSystemDefaultDevice() };
            unsafe { Retained::retain(ptr) }.expect("Failed to get default system device.")
        };

        // create the command queue
        let command_queue = device
            .newCommandQueue()
            .expect("Failed to create a command queue.");

        // create the metal view
        let mtk_view = {
            let frame_rect = window.frame();
            unsafe { MTKView::initWithFrame_device(mtm.alloc(), frame_rect, Some(&device)) }
        };

        // create the pipeline descriptor
        let pipeline_descriptor = MTLRenderPipelineDescriptor::new();

        unsafe {
            pipeline_descriptor
                .colorAttachments()
                .objectAtIndexedSubscript(0)
                .setPixelFormat(mtk_view.colorPixelFormat());
        }

        // compile the shaders
        let library = device
            .newLibraryWithSource_options_error(
                ns_string!(include_str!("triangle.metal")),
                None,
            )
            .expect("Failed to create a library.");

        // configure the vertex shader
        let vertex_function = library.newFunctionWithName(ns_string!("vertex_main"));
        pipeline_descriptor.setVertexFunction(vertex_function.as_deref());

        // configure the fragment shader
        let fragment_function = library.newFunctionWithName(ns_string!("fragment_main"));
        pipeline_descriptor.setFragmentFunction(fragment_function.as_deref());

        // create the pipeline state
        let pipeline_state = device
            .newRenderPipelineStateWithDescriptor_error(&pipeline_descriptor)
            .expect("Failed to create a pipeline state.");

        // configure the metal view delegate
        unsafe {
            let object = ProtocolObject::from_ref(self);
            mtk_view.setDelegate(Some(object));
        }

        // configure the window
        let view = window.contentView().unwrap();
        unsafe {
            view.addSubview(&mtk_view);
            mtk_view.setFrame(view.frame());
        }

        //window.setContentView(Some(&mtk_view));
        window.center();
        window.setTitle(ns_string!("Metal Example"));

        // initialize the delegate state
        self.ivars().command_queue.set(command_queue).expect("Failed to set command queue.");
        self.ivars().pipeline_state.set(pipeline_state).expect("Failed to set pipeline state.");
        self.ivars().mtk_view.set(mtk_view).expect("Failed to set mtk_view.");
    }

    fn new(tao_window: &Window) -> Retained<Self> {
        let ns_window = tao_window.ns_window() as *mut NSWindow;
        let window;
        unsafe {
            window = Retained::from_raw(ns_window).unwrap();
        }

        let mtm = MainThreadMarker::new().unwrap();
        let this = mtm.alloc();

        // initialize the delegate state
        let this = this.set_ivars(AppState {
            command_queue: OnceCell::default(),
            pipeline_state: OnceCell::default(),
            window: OnceCell::from(window),
            mtk_view: OnceCell::new(),
        });

        unsafe { msg_send_id![super(this), init] }
    }
}

#[allow(clippy::single_match)]
#[allow(clippy::collapsible_match)]
fn main() {
    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("A fantastic window!")
        .build(&event_loop)
        .unwrap();

    let mtk_view_delegate = MtkViewDelegate::new(&window);
    mtk_view_delegate.init();

    event_loop.run(move |event, _, control_flow| {
        //println!("{event:?}");

        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(size) => {
                    let mtk_view = mtk_view_delegate.ivars().mtk_view.get().unwrap();
                    let ns_window = mtk_view_delegate.ivars().window.get().unwrap();
                    unsafe {
                        mtk_view.setFrame(ns_window.contentView().unwrap().frame());
                    }
                }
                _ => (),
            },
            Event::RedrawRequested(_) => {
                //window.request_redraw();
            }
            _ => (),
        }
    });
}