use core::str;
use std::io::BufRead;
use std::io::Cursor;
use std::io::BufReader;

use my_web_app::CountFileMetaColumnData;
use my_web_app::ReductionResponse;
use serde::Deserialize;
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::window;
use web_sys::{DomRect, EventTarget, HtmlElement, HtmlCanvasElement, CanvasRenderingContext2d, WebGlRenderingContext as GL};
use yew::context;
use yew::{html, Callback, Component, Context, Html, MouseEvent, NodeRef, WheelEvent};
use yew::Properties;
use std::f64;

use crate::appstate::AsyncData;
use crate::appstate::PerCellDataSource;
use crate::camera::Camera2D;
use crate::camera::Rectangle2D;
use crate::histogram::make_safe_minmax;
use crate::resize::ComponentSize;
use crate::closestpoint::ClosestPointIndex2D;


// see https://github.com/yewstack/yew/blob/master/examples/webgl/src/main.rs


////////////////////////////////////////////////////////////
/// RGB color, 0...1
type Color3f = (f32,f32,f32);

////////////////////////////////////////////////////////////
/// Vectors, 3d and 4d
type Vec3 = (f32,f32,f32);

////////////////////////////////////////////////////////////
/// Coloring of the reduction
#[derive(PartialEq, Clone)]
pub enum ReductionColoring {
    None,
    ByMeta(PerCellDataSource),   //////////// this datastructure is not really needed => option
}

////////////////////////////////////////////////////////////
/// Coloring of the reduction
#[derive(PartialEq, Clone, Debug)]
pub enum ReductionColoringWithData {
    None,
    ByMeta(PerCellDataSource, AsyncData<CountFileMetaColumnData>), //////////// this datastructure is not really needed => option
}

////////////////////////////////////////////////////////////
/// Coordinates for a reduction
#[derive(Debug, Deserialize, Serialize)]
pub struct ReductionViewData {
    pub num_point: usize,
    pub data: Vec<f32>,
    //pub ids: Vec<String>, //cluster_id

    pub max_x: f32,
    pub max_y: f32,
    pub min_x: f32,
    pub min_y: f32,
}
    //    keep this in a cache? x,y and xy together??



////////////////////////////////////////////////////////////
/// Convert from a reduction server response to a optimized data structure
pub fn convert_from_response_to_reduction_data(resp: ReductionResponse) -> ReductionViewData {

    let num_point= resp.x.len();

    //Figure out reduction point range
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;

    resp.x.iter().for_each(|v|{
        max_x = max_x.max(*v);
        min_x = min_x.min(*v);
    });

    resp.y.iter().for_each(|v|{
        max_y = max_y.max(*v);
        min_y = min_y.min(*v);
    });

    //Convert coordinates to flat list. better to send in this format already?  --- code is likely fairly slow in current design
    let mut data:Vec<f32> = Vec::with_capacity(num_point*2);
    unsafe {
        data.set_len(num_point*2);
    }

    resp.x.iter().enumerate().for_each(|(i,v)| {
        data[i*2] = *v;
    });

    resp.y.iter().enumerate().for_each(|(i,v)| {
        data[i*2+1] = *v;
    });

    /*
    is above faster? it should eliminate a bound check at minimum. but would be great if we could instead do below unsafely
    for i in 0..num_point {
        data[i*2] = resp.x[i];
        data[i*2+1] = resp.y[i];
    }
     */

    ReductionViewData {
        num_point: num_point,
        data: data,
        max_x: max_x,
        max_y: max_y,
        min_x: min_x,
        min_y: min_y
    }
}






////////////////////////////////////////////////////////////
/// Enum for the currently selected tool
#[derive(Debug, PartialEq)]
pub enum CurrentTool {
    Zoom,
    ZoomAll,
    Select
}


////////////////////////////////////////////////////////////
/// Message sent to the event system for updating the page
#[derive(Debug)]
pub enum MsgReduction {
    MouseMove(f32,f32, bool),
    MouseClick,
    MouseWheel(f32),
    MouseStartSelect(f32,f32),
    MouseEndSelect(f32,f32),
    SelectCurrentTool(CurrentTool),
}


////////////////////////////////////////////////////////////
/// Properties for ReductionView
#[derive(Properties, PartialEq)]
pub struct Props {
    pub on_cell_hovered: Callback<Option<usize>>,
    pub on_cell_clicked: Callback<Vec<usize>>,
    pub reduction_data: AsyncData<ReductionViewData>, 
    pub color_reduction_by: ReductionColoringWithData,
    pub last_component_size: ComponentSize,
     pub current_colorby: PerCellDataSource,
}


////////////////////////////////////////////////////////////
/// random note: Wrap gl in Rc (Arc for multi-threaded) so it can be injected into the render-loop closure.
pub struct ReductionView {
    node_refs: Vec<NodeRef>,
    last_pos: (f32,f32),
    last_cell: Option<usize>,
    closest_point_index: ClosestPointIndex2D,
    current_tool: CurrentTool,
    camera: Camera2D,
    current_selection: Option<Rectangle2D>,
    last_reduction_data: AsyncData<ReductionViewData>,
}

impl Component for ReductionView {
    type Message = MsgReduction;
    type Properties = Props;
    
    ////////////////////////////////////////////////////////////
    /// Create this component
    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            node_refs:vec![NodeRef::default(), NodeRef::default()],
            last_pos: (0.0,0.0),
            last_cell: None,
            closest_point_index: ClosestPointIndex2D::new(), //tricky... adapt to umap size??
            current_tool: CurrentTool::Select,
            camera: Camera2D::new(),
            current_selection: None,
            last_reduction_data: AsyncData::NotLoaded,
        }
    }


    ////////////////////////////////////////////////////////////
    /// Handle an update message
    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {            

            ////////////////////////////////////////////////////////////
            // Message: Mouse has moved
            MsgReduction::MouseMove(x,y, press_left) => {
                let mut do_update = false;
                let last_pos = self.last_pos;
                self.last_pos = (x,y);
                //  log::debug!(".. {:?}", last_pos);

                //Handle pointer in world coordinates
                let (wx,wy) = self.camera.cam2world(x as f32, y as f32);

                //Handle hovering
                let cp = self.closest_point_index.get_closest_point(wx, wy);  // sometimes a crash overflow here?? 666
                //log::debug!("p: {:?}",cp);
                //log::debug!("{} {}",x,y);

                let point_name = cp;
                /*
                if let Some(umap) = &self.umap {
                    if let Some(cp) = cp {
                        point_name = Some(umap.ids.get(cp).unwrap().clone());                      
                    }
                }
                 */
                
                //If we hover a new point, emit signal
                let point_changed = self.last_cell != point_name;
                self.last_cell = point_name.clone();
                if point_changed {
                    ctx.props().on_cell_hovered.emit(point_name);
                    do_update=true;
                }

                if let Some(sel) = &mut self.current_selection {
                    sel.x2=wx;
                    sel.y2=wy;
                    //log::debug!("sel-move {:?}",sel);
                }

                //Handle panning
                if self.current_tool == CurrentTool::Zoom && press_left {
                    let dx = x - last_pos.0;
                    let dy = y - last_pos.1;
                    //log::debug!("dd {:?}", (dx,dy));
                    self.camera.x -= (dx as f32) / self.camera.zoom_x;
                    self.camera.y -= (dy as f32) / self.camera.zoom_y;
                    return true;
                }

                //Always update view if a selection is going on
                if let Some(_sel) = &self.current_selection {
                    do_update=true;
                }
                do_update
            },

            ////////////////////////////////////////////////////////////
            // Message: Mouse wheel rotated
            MsgReduction::MouseWheel(dy) => {
                let (cx,cy) = self.last_pos;
                let (wx, wy) = self.camera.cam2world(cx, cy);
                let scale = (10.0f32).powf(dy / 100.0);
                self.camera.zoom_around(wx,wy, scale);
                true
            },

            ////////////////////////////////////////////////////////////
            // Message: Mouse has clicked
            MsgReduction::MouseClick => {
                false
            },

            ////////////////////////////////////////////////////////////
            // Message: A tool has been selected
            MsgReduction::SelectCurrentTool(t) => {

                let reduction_data = &ctx.props().reduction_data;

                if t==CurrentTool::ZoomAll {
                    if let AsyncData::Loaded(reduction_data) = reduction_data {
                        self.camera.fit_reduction(reduction_data);
                    }
                } else {
                    self.current_tool=t;
                }
                true
            },

            ////////////////////////////////////////////////////////////
            // Message: A selection of a region has started using mouse
            MsgReduction::MouseStartSelect(cx,cy) => {
                if self.current_tool==CurrentTool::Select {
                    let (wx,wy) = self.camera.cam2world(cx as f32, cy as f32);
                    self.current_selection = Some(Rectangle2D {
                        x1: wx,
                        x2: wx,
                        y1: wy,
                        y2: wy
                    });
                    //log::debug!("sel-start {:?}",self.current_selection);
                    true
                } else {
                    false
                }
            },

            ////////////////////////////////////////////////////////////
            // Message: A selection of a region has ended using mouse
            MsgReduction::MouseEndSelect(cx,cy) => {
                if let Some(rect) = &mut self.current_selection {
                    let (wx,wy) = self.camera.cam2world(cx as f32, cy as f32);
                    rect.x2=wx;
                    rect.y2=wy;

                    let reduction_data = &ctx.props().reduction_data;

                    if let AsyncData::Loaded(reduction_data) = reduction_data {

                        let (x1,x2) =rect.range_x();
                        let (y1,y2) =rect.range_y();

                        if x1==x2 && y1==y2 {
                            log::debug!("this is a click");
                            if self.current_tool==CurrentTool::Select {
                                if let Some(cell) = &self.last_cell {
                                    ctx.props().on_cell_clicked.emit(vec![cell.clone()]);
                                }
                            }
                        } else {
                            log::debug!("this is a rect select");
                            //log::debug!("wrect {} -- {}     {} -- {}", x1,x2,    y1,y2);

                            //Scan all points to see if they are within the selection 
                            let mut selected_vert = Vec::new();
                            let num_points = reduction_data.num_point;
                            let vertices = &reduction_data.data;    
                            for i in 0..num_points {
                                let px = *vertices.get(i*2+0).unwrap();
                                let py = *vertices.get(i*2+1).unwrap();
                                //log::debug!("{} {}", px, py);
                                if px>x1 && px<x2 && py>y1 && py<y2 { /////////////////////// TODO - invert y axis??   ////////////////// points halfway down are at y=500
                                    let point_name = i;
                                    //let point_name = umap.ids.get(i).unwrap().clone();
                                    selected_vert.push(point_name);
                                }
                            }
                            //log::debug!("sel-end {:?}",rect);
                            //log::debug!("sel-en!! {:?}",selected_vert);

                            ctx.props().on_cell_clicked.emit(selected_vert);                            
                        }
                    }
                    self.current_selection=None;
                }
                true
            }

        }
    }




    ////////////////////////////////////////////////////////////
    /// Render this component
    fn view(&self, ctx: &Context<Self>) -> Html {
       let current_legend: PerCellDataSource = ctx.props().current_colorby.clone();
       let legend_name = match current_legend {
           PerCellDataSource::Metadata(name) => name,
           PerCellDataSource::Counts(_,_) => "Error_naming_legend".to_string()
       };

        let cb_mousemoved = ctx.link().callback(move |e: MouseEvent | { 
            e.prevent_default();
            let (x_cam, y_cam) = mouseevent_get_cx(&e);
            let press_left = e.buttons() & 1 > 0;

            MsgReduction::MouseMove(x_cam,y_cam, press_left)
            //there is mouse movement! https://developer.mozilla.org/en-US/docs/Web/API/MouseEvent/movementX 
        });
        
        let cb_mousewheel = ctx.link().callback(move |e: WheelEvent | { 
            e.prevent_default();
            MsgReduction::MouseWheel(e.delta_y() as f32)
        });

        let cb_mouseclicked = ctx.link().callback(move |_e: MouseEvent | { 
            MsgReduction::MouseClick
        });
        
        let cb_click_select = ctx.link().callback(move |_e: MouseEvent | { 
            MsgReduction::SelectCurrentTool(CurrentTool::Select)
        });

        let cb_click_zoom = ctx.link().callback(move |_e: MouseEvent | { 
            MsgReduction::SelectCurrentTool(CurrentTool::Zoom)
        });

        let cb_click_zoomall = ctx.link().callback(move |_e: MouseEvent | { 
            MsgReduction::SelectCurrentTool(CurrentTool::ZoomAll)
        });

        let cb_onmousedown = ctx.link().callback(move |e: MouseEvent | { 
            e.prevent_default();
            let (x_cam, y_cam) = mouseevent_get_cx(&e);
            MsgReduction::MouseStartSelect(x_cam, y_cam)
        });

        let cb_onmouseup = ctx.link().callback(move |e: MouseEvent | { 
            e.prevent_default();
            let (x_cam, y_cam) = mouseevent_get_cx(&e);
            MsgReduction::MouseEndSelect(x_cam, y_cam)
        });

        // Render box representing current selection
        let html_select = if let Some(rect) = &self.current_selection {

            let (x1,x2) = rect.range_x();
            let (y1,y2) = rect.range_y();

            let (x1,y1) = self.camera.world2cam(x1, y1); //camera is in range [-1,1]
            let (x2,y2) = self.camera.world2cam(x2, y2);

            let canvas = self.node_refs[0].cast::<HtmlCanvasElement>().unwrap();
            let w = canvas.width() as f32;
            let h = canvas.height() as f32;

            let x1 = x1*w/2.0 + w/2.0;
            let x2 = x2*w/2.0 + w/2.0;
            let y1 = y1*h/2.0 + h/2.0;
            let y2 = y2*h/2.0 + h/2.0;

            html! {
                <rect x={x1.to_string()} y={y1.to_string()} width={(x2-x1).to_string()} height={(y2-y1).to_string()}    fill-opacity="0.1" fill="blue" stroke-width="2" stroke="black" stroke-dasharray="5,5"/> //fillstyle="fill:rgba(0,0,0,0.1);stroke-width:1;"
            }
        } else {
            html! {""}
        };

        //Compute current canvas size. Not automatic via CSS
        let window = window().expect("no window");//.document().expect("no document on window");
        let _window_h = window.inner_height().expect("failed to get height").as_f64().unwrap();
        let window_w = window.inner_width().expect("failed to get width").as_f64().unwrap();
        let canvas_w = (window_w*0.59) as usize;
        let canvas_h = 500 as usize; //(window_h*0.59) as usize;
        
        //Compose the view
        html! {
            <div style="display: flex; height: 500px; position: relative;">

                <div style="position: absolute; left:0; top:0; display: flex; ">
                    <canvas 
                        ref={self.node_refs[0].clone()} 
                        style="border:1px solid #000000;"
                        onmousemove={cb_mousemoved} onclick={cb_mouseclicked} onwheel={cb_mousewheel} onmousedown={cb_onmousedown} onmouseup={cb_onmouseup}
                        width={format!{"{}", canvas_w}}
                        height={format!{"{}", canvas_h}}
                    />
                </div>

                //Overlay SVG
                <div style="position: absolute; left:0; top:0; display: flex; pointer-events: none; ">  
                    <svg style={format!("width: {}px; height: {}px; pointer-events: none;", canvas_w, canvas_h)}> // note: WxH must cover canvas!!  
                        { html_select }
                    </svg>
                </div>
                
                // Button: Select
                <div style={get_tool_style(canvas_w-40, self.current_tool==CurrentTool::Select)} onclick={cb_click_select}>
                    <svg data-icon="polygon-filter" height="16" role="img" viewBox="0 0 16 16" width="16"><path d="M14 5c-.24 0-.47.05-.68.13L9.97 2.34c.01-.11.03-.22.03-.34 0-1.1-.9-2-2-2S6 .9 6 2c0 .04.01.08.01.12L2.88 4.21C2.61 4.08 2.32 4 2 4 .9 4 0 4.9 0 6c0 .74.4 1.38 1 1.72v4.55c-.6.35-1 .99-1 1.73 0 1.1.9 2 2 2 .74 0 1.38-.4 1.72-1h4.55c.35.6.98 1 1.72 1 1.1 0 2-.9 2-2 0-.37-.11-.7-.28-1L14 9c1.11-.01 2-.9 2-2s-.9-2-2-2zm-4.01 7c-.73 0-1.37.41-1.71 1H3.73c-.18-.3-.43-.55-.73-.72V7.72c.6-.34 1-.98 1-1.72 0-.04-.01-.08-.01-.12l3.13-2.09c.27.13.56.21.88.21.24 0 .47-.05.68-.13l3.35 2.79c-.01.11-.03.22-.03.34 0 .37.11.7.28 1l-2.29 4z" fill-rule="evenodd"></path></svg>
                </div>

                // Button: Zoom
                <div style={get_tool_style(canvas_w-40-30, self.current_tool==CurrentTool::Zoom)} onclick={cb_click_zoom}>
                    <svg data-icon="zoom-in" height="16" role="img" viewBox="0 0 16 16" width="16"><path d="M7.99 5.99v-2c0-.55-.45-1-1-1s-1 .45-1 1v2h-2c-.55 0-1 .45-1 1s.45 1 1 1h2v2c0 .55.45 1 1 1s1-.45 1-1v-2h2c.55 0 1-.45 1-1s-.45-1-1-1h-2zm7.56 7.44l-2.67-2.68a6.94 6.94 0 001.11-3.76c0-3.87-3.13-7-7-7s-7 3.13-7 7 3.13 7 7 7c1.39 0 2.68-.42 3.76-1.11l2.68 2.67a1.498 1.498 0 102.12-2.12zm-8.56-1.44c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5z" fill-rule="evenodd"></path></svg>
                </div>

                // Button: Zoom all
                <div style={get_tool_style(canvas_w-40-30-30, self.current_tool==CurrentTool::ZoomAll)} onclick={cb_click_zoomall}>
                    <svg data-icon="zoom-in" height="16" width="16" xmlns="http://www.w3.org/2000/svg"><path style="fill:none;stroke:#000;stroke-width:2.01074px;stroke-linecap:butt;stroke-linejoin:miter;stroke-opacity:1" d="M14.733 8.764v5.973H9.586m-8.29-5.973v5.973h5.146m8.29-7.5V1.264H9.587m-8.29 5.973V1.264h5.146"/></svg>
                </div>
                 <div id = "continuous_var_legend" style="position: absolute; left: 8px; top: 55px; z-index: 1; pointer-events: none; height: 200px; width: 80px;">
                 <canvas ref={self.node_refs[1].clone()} height = "180" width = "20" style="position: absolute; left: 0px; top: 17px;" id = "legend_canvas">
                 </canvas>
                  <svg height="200px" width="80px" style="position: absolute; left: 0px; top: 0px;">
                   <path d="M 20 10 H 19 V 200 Z" stroke="black" />
                 <text id="continuous_var_label" transform="rotate(-90)" y="2" x="-100" dy="1em" data-testid="continuous_legend_color_by_label" aria-label="nCount_RNA" style="text-anchor: middle; fill: white; padding: 2px;">{legend_name}</text>
                 </svg>
                 </div>

            </div>
        }
    }



    ////////////////////////////////////////////////////////////
    /// Called after DOM has been created
    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        let reduction_data = &ctx.props().reduction_data;

        if let AsyncData::Loaded(datapoints) = reduction_data {

            //Fit camera whenever we get a new umap to show
            if self.last_reduction_data != *reduction_data {
                self.camera.fit_reduction(datapoints);
            }
            self.last_reduction_data = reduction_data.clone();


            // Only start the render loop if it's the first render
            // There's no loop cancellation taking place, so if multiple renders happen,
            // there would be multiple loops running. That doesn't *really* matter here because
            // there's no props update and no SSR is taking place, but it is something to keep in
            // consideration

            // TODO should we only render if data changed?
            /*
            if !first_render {
                return;
            }
            */
            

            // Once rendered, store references for the canvas and GL context. These can be used for
            // resizing the rendering area when the window or canvas element are resized, as well as
            // for making GL calls.
            let canvas = self.node_refs[0].cast::<HtmlCanvasElement>().unwrap();

            let gl: GL = canvas
                .get_context("webgl")
                .unwrap()
                .unwrap()
                .dyn_into()
                .unwrap();

            let vert_code = String::from(include_str!("./umap.vert"));
            let frag_code = include_str!("./umap.frag");

            //Get position data
            let num_points = datapoints.num_point;
            let vertices = &datapoints.data;    
            let mut vec_vertex:Vec<f32> = Vec::new();

            let vec_vertex_size = 6;
            vec_vertex.reserve(num_points*6);  //Size of vec3+vec3
            for i in 0..num_points {
                let input_base = i*2;
                vec_vertex.push(*vertices.get(input_base+0).unwrap());
                vec_vertex.push(*vertices.get(input_base+1).unwrap());
                vec_vertex.push(0.0); // only used for 3d reductions

                vec_vertex.push(0.0); ///////////////////////////////////////////////// color index. remove, put in separate buffer
                vec_vertex.push(0.0); ///////////////////////////////////////////////// color index. remove, put in separate buffer    filler for now
                vec_vertex.push(0.0); ///////////////////////////////////////////////// color index. remove, put in separate buffer
            }

            //Get color data
            let color_reduction_by = &ctx.props().color_reduction_by;
            log::debug!("Rendering {:?}",color_reduction_by);
            if let ReductionColoringWithData::ByMeta(_name, color_data) = color_reduction_by {
                if let AsyncData::Loaded(color_data) = color_data {
                    match color_data.as_ref() {

                        ///////// Color by categorical data
                        CountFileMetaColumnData::Categorical(vec_data, vec_cats) => {
                            //log::debug!("Making colors for category");
                            
                            //let palette = self.color_dict.get("default").unwrap();
                            let palette = get_palette_for_categories(vec_cats.len());

                            for (i,p) in vec_data.iter().enumerate() {
                                let col = palette.get((*p as usize) % palette.len()).unwrap();
                                let base = vec_vertex_size*i;
                                vec_vertex[base + 3] = col.0;
                                vec_vertex[base + 4] = col.1;
                                vec_vertex[base + 5] = col.2;

                            }

                        },

                        ///////// Color by numerical data - plain array
                        CountFileMetaColumnData::Numeric(vec_data) => {

                            //Normalize color range. TODO should only need to do this once during loading
                            let (_min_val, max_val) = make_safe_minmax(&vec_data);
                            for (i,p) in vec_data.into_iter().enumerate() {
                                let base = vec_vertex_size*i;
                                vec_vertex[base + 3] = p/max_val;
                                vec_vertex[base + 4] = 0.0;
                                vec_vertex[base + 5] = 0.0;
                            }

                            let max_cont_val: f32 = max_val;
                            log::debug!("Max num {}", max_cont_val);

            
                            let document = web_sys::window().unwrap().document().unwrap();
                            log::debug!("{:?}", document);
                            let canvas = document.get_element_by_id("legend_canvas").unwrap();
                            log::debug!("{:?}", web_sys::Element::get_attribute_names(&canvas));
                            let canvas: web_sys::HtmlCanvasElement = canvas
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap();
    let context = canvas
        .get_context("2d")
        .unwrap()
        .unwrap()
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .unwrap();

    log::debug!("{:?}", web_sys::CanvasRenderingContext2d::stroke_style(&context));
    context.begin_path();

    // Draw the outer circle.
    context
        .round_rect(5.0, 0.0, 15.0, 180.0)
        .unwrap();
    context.stroke();

    #[wasm_bindgen(module = "/src/color_legend_gradient.js")]
    extern "C" {
         fn color_gradient(context: CanvasRenderingContext2d) -> CanvasRenderingContext2d;
    }
    let context = color_gradient(context);
    context.fill();
    


/* 
                         let legend_vert_code = String::from(include_str!("./legend_bar.vert"));
                        let legend_frag_code = include_str!("./legend_bar.frag");

                        let legend_bar = self.node_refs[1].cast::<HtmlCanvasElement>().unwrap();
                        let gl_legend: GL = legend_bar
                        .get_context("webgl")
                        .unwrap()
                        .unwrap()
                        .dyn_into()
                        .unwrap();

                        gl_legend.clear_color(5.0, 3.0, 0.0, 1.0);
                        gl_legend.clear(GL::COLOR_BUFFER_BIT);

                        let legend_vert_shader = gl_legend.create_shader(GL::VERTEX_SHADER).unwrap();
                         gl_legend.shader_source(&legend_vert_shader, legend_vert_code.as_str());
                          gl_legend.compile_shader(&legend_vert_shader);

                          let legend_frag_shader = gl_legend.create_shader(GL::FRAGMENT_SHADER).unwrap();
                        gl_legend.shader_source(&legend_frag_shader, legend_frag_code);
                          gl_legend.compile_shader(&legend_frag_shader);

                          //Attach shaders
                    let legend_shader_program = gl_legend.create_program().unwrap();
                    gl_legend.attach_shader(&legend_shader_program, &legend_vert_shader);
                    gl_legend.attach_shader(&legend_shader_program, &legend_frag_shader);
                    gl_legend.link_program(&legend_shader_program);
                    gl_legend.use_program(Some(&legend_shader_program));

                    //Attach the position vector as an attribute for the GL context.
                    let a_position = gl_legend.get_attrib_location(&legend_shader_program, "a_position") as u32;

                    let positionBuffer = gl_legend.create_buffer().unwrap();
                    gl_legend.bind_buffer(GL::ARRAY_BUFFER, Some(&positionBuffer));

                    let positions: Vec<f32> = vec![
                    -1.0, -1.0, 
                    1.0, -1.0, 
                    -1.0, 1.0, 
                    -1.0, 1.0, 
                    1.0, -1.0, 
                    1.0, 1.0,
                ];
       
                let verts = js_sys::Float32Array::from(positions.as_slice());
                gl_legend.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &verts, GL::STATIC_DRAW);

                gl_legend.vertex_attrib_pointer_with_i32(a_position, 2, GL::FLOAT, false, 0, 0);
                gl_legend.enable_vertex_attrib_array(a_position);

                gl_legend.draw_arrays(GL::TRIANGLES, 0, 6); */

                        },

                        ///////// Color by numerical data - sparse array
                        CountFileMetaColumnData::SparseNumeric(vec_index, vec_data) => {

                            //Normalize color range. TODO should only need to do this once during loading. note, for sparse, min_val should be 0 by definition, more or less
                            let (min_val, max_val) = make_safe_minmax(&vec_data);
                            log::debug!("Render value range {} {}",min_val, max_val);

                            for (i,p) in vec_index.iter().zip(vec_data.iter()) {
                                let i = *i as usize;
                                let base = vec_vertex_size*i;
                                vec_vertex[base + 3] = p/max_val;
                                vec_vertex[base + 4] = 0.0;
                                vec_vertex[base + 5] = 0.0;
                            }
                        },
                    }
                }
            } else {
                // Put in an empty color (default is black now)
            }

            //Connect vertex array to GL
            let vertex_buffer = gl.create_buffer().unwrap();
            let js_vertex = js_sys::Float32Array::from(vec_vertex.as_slice());
            //let verts = js_sys::Int32Array::from(vertices_int.as_slice());
            gl.bind_buffer(GL::ARRAY_BUFFER, Some(&vertex_buffer));
            gl.buffer_data_with_array_buffer_view(GL::ARRAY_BUFFER, &js_vertex, GL::STATIC_DRAW);

            //Compile vertex shader
            let vert_shader = gl.create_shader(GL::VERTEX_SHADER).unwrap();
            gl.shader_source(&vert_shader, vert_code.as_str());
            gl.compile_shader(&vert_shader);

            
            /*let msg= gl.get_shader_info_log(&vert_shader);
            if let Some(msg)=msg {
                log::debug!("error {}", msg);
            }*/

            //Compile fragment shader
            let frag_shader = gl.create_shader(GL::FRAGMENT_SHADER).unwrap();
            gl.shader_source(&frag_shader, frag_code);
            gl.compile_shader(&frag_shader);

            //Attach shaders
            let shader_program = gl.create_program().unwrap();
            gl.attach_shader(&shader_program, &vert_shader);
            gl.attach_shader(&shader_program, &frag_shader);
            gl.link_program(&shader_program);
            gl.use_program(Some(&shader_program));

            //Size of a float in bytes
            let sizeof_float = 4;

            //Attach the position vector as an attribute for the GL context.
            let a_position = gl.get_attrib_location(&shader_program, "a_position") as u32;
            //log::debug!("a_position {}",a_position);
            gl.enable_vertex_attrib_array(a_position);
            gl.vertex_attrib_pointer_with_i32(a_position, 3, GL::FLOAT, false, sizeof_float*6, 0);  

            //Attach color vector as an attribute
            let a_color = gl.get_attrib_location(&shader_program, "a_color") as u32;
            //log::debug!("a_color {}",a_color);
            gl.enable_vertex_attrib_array(a_color);
            gl.vertex_attrib_pointer_with_i32(a_color, 3, GL::FLOAT, false, sizeof_float*6, sizeof_float*3);   //index of out range   ... not big enough for the draw call

            //Attach camera attributes
            let u_camera_x = gl.get_uniform_location(&shader_program, "u_camera_x");
            let u_camera_y = gl.get_uniform_location(&shader_program, "u_camera_y");
            let u_camera_zoom_x = gl.get_uniform_location(&shader_program, "u_camera_zoom_x");
            let u_camera_zoom_y = gl.get_uniform_location(&shader_program, "u_camera_zoom_y");
            gl.uniform1f(u_camera_x.as_ref(), self.camera.x as f32);
            gl.uniform1f(u_camera_y.as_ref(), self.camera.y as f32);
            gl.uniform1f(u_camera_zoom_x.as_ref(), self.camera.zoom_x as f32);
            gl.uniform1f(u_camera_zoom_y.as_ref(), self.camera.zoom_y as f32);

            //log::debug!("canvas {} {}   {:?}", canvas.width(), canvas.height(), self.camera);

            let u_display_w = gl.get_uniform_location(&shader_program, "u_display_w");
            let u_display_h = gl.get_uniform_location(&shader_program, "u_display_h");
            gl.uniform1f(u_display_w.as_ref(), canvas.width() as f32);
            gl.uniform1f(u_display_h.as_ref(), canvas.height() as f32);

            // clear canvas
            gl.clear_color(1.0, 1.0, 1.0, 1.0);
            gl.clear(GL::COLOR_BUFFER_BIT);
            
            // to make round points, need to draw square https://stackoverflow.com/questions/7237086/opengl-es-2-0-equivalent-for-es-1-0-circles-using-gl-point-smooth
            gl.draw_arrays(GL::POINTS, 0, num_points as i32);
        }

    }
}





////////////////////////////////////////////////////////////
/// Convert from vector to HTML color code
pub fn rgbvec2string(c: Vec3) -> String {
    let red=(c.0*255.0) as u8;
    let green=(c.1*255.0) as u8;
    let blue=(c.2*255.0) as u8;
    format!("#{:02X}{:02X}{:02X}", red, green, blue)
}



////////////////////////////////////////////////////////////
/// Get current camera position from a mouse event
fn mouseevent_get_cx(e: &MouseEvent) -> (f32,f32) {
    let target: Option<EventTarget> = e.target();
    let canvas: HtmlCanvasElement = target.and_then(|t| t.dyn_into::<HtmlCanvasElement>().ok()).expect("wrong type");

    let rect:DomRect = canvas.get_bounding_client_rect();
    let x = e.client_x() - (rect.left() as i32);
    let y = e.client_y() - (rect.top() as i32);

    let w = rect.width() as f32;
    let h = rect.height() as f32;

    let x_cam = (x as f32 - w/2.0)/(w/2.0);
    let y_cam = (y as f32 - h/2.0)/(h/2.0);

//    log::debug!("getcx  {} {}", x_cam, y_cam);

    (x_cam, y_cam)
}



////////////////////////////////////////////////////////////
/// Read color RGB vector from html string to 0..255
pub fn parse_rgb_i64(s: &String) -> (i64, i64, i64) {

    let s = s.as_str();
    let s_r = s.get(1..3).expect("Could not get R");
    let s_g = s.get(3..5).expect("Could not get G");
    let s_b = s.get(5..7).expect("Could not get B");
    //log::debug!("got r: {} {} {}",s_r, s_g, s_b);

    let r = i64::from_str_radix(s_r, 16).expect("parse error");
    let g = i64::from_str_radix(s_g, 16).expect("parse error");
    let b = i64::from_str_radix(s_b, 16).expect("parse error");

    (r,g,b)
}


////////////////////////////////////////////////////////////
/// Read color RGB vector from html string to 0..1
pub fn parse_rgb_f64(s: &String) -> (f32, f32, f32) {
    let (r,g,b) = parse_rgb_i64(s);
    (
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
    )
}


////////////////////////////////////////////////////////////
/// Generate palette info
pub fn parse_palette(csv_colors:&str) -> Vec<(f32,f32,f32)> {
    let mut list_colors = Vec::new();
    let palette = Cursor::new(csv_colors);
    let reader = BufReader::new(palette);
    for line in reader.lines() {
        let line=line.unwrap();
        let rgb_color = parse_rgb_f64(&line);
        list_colors.push(rgb_color);
    }
    list_colors
}


////////////////////////////////////////////////////////////
/// Get palette suitable for the given categories
pub fn get_palette_for_categories(_num_cats: usize) -> Vec<Color3f> {
//    let palette = self.color_dict.get("default").unwrap();
    let pal = parse_palette(include_str!("./palette.csv"));
    pal
}


////////////////////////////////////////////////////////////
/// Get the style of a tool button
fn get_tool_style(pos: usize, selected: bool) -> String {
    let c=if selected {"#0099FF"} else {"lightgray"};
    format!("position: absolute; left:{}px; top:10px; display: flex; border-radius: 3px; border: 2px solid gray; padding: 5px; background-color: {};", pos, c)
}