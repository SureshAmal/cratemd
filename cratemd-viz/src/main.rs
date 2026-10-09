mod db;
mod graph;
mod loader;

#[cfg(test)]
mod graph_tests;

use eframe::egui::{self, Color32, FontId, Pos2, Stroke, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use petgraph::graph::NodeIndex;
use std::path::PathBuf;
use std::time::Instant;

use crate::db::{MemoryNote, MetaEntry, VizDb};
use crate::graph::VizGraph;
use crate::loader::ProjectLoader;

struct CratemdVizApp {
    db_path: PathBuf,
    db: Option<VizDb>,
    graph: VizGraph,
    memories: Vec<MemoryNote>,
    meta: Vec<MetaEntry>,

    // Search and filter state
    query: String,
    current_focus: Option<String>,
    history: Vec<String>,
    selected_node: Option<NodeIndex>,
    dragged_node: Option<NodeIndex>,
    last_frame_time: Instant,

    // UI and Markdown state
    markdown_cache: CommonMarkCache,
    status_message: String,
    physics_enabled: bool,
    zoom: f32,
    pan: Vec2,
}

impl CratemdVizApp {
    fn new(_cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        let mut app = Self {
            db_path: initial_path.unwrap_or_else(|| PathBuf::from(".")),
            db: None,
            graph: VizGraph::new(),
            memories: Vec::new(),
            meta: Vec::new(),
            query: String::new(),
            current_focus: None,
            history: Vec::new(),
            selected_node: None,
            dragged_node: None,
            last_frame_time: Instant::now(),
            markdown_cache: CommonMarkCache::default(),
            status_message: "Initializing cratemd visualizer...".to_string(),
            physics_enabled: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
        };

        app.reload_data();
        app
    }

    fn drill_down(&mut self, target_id: String) {
        if let Some(prev) = self.current_focus.take() {
            self.history.push(prev);
        } else {
            self.history.push("root".to_string());
        }
        self.current_focus = Some(target_id);
        self.selected_node = None;
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.reload_data();
    }

    fn navigate_back(&mut self) {
        if let Some(prev) = self.history.pop() {
            if prev == "root" {
                self.current_focus = None;
            } else {
                self.current_focus = Some(prev);
            }
        } else {
            self.current_focus = None;
        }
        self.selected_node = None;
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.reload_data();
    }

    fn reload_data(&mut self) {
        match VizDb::open(&self.db_path) {
            Ok(db) => {
                let memories = db.search_memories(&self.query).unwrap_or_default();
                let meta = db.load_meta().unwrap_or_default();

                if let Err(e) = ProjectLoader::populate_graph(
                    &mut self.graph,
                    &db.db_path,
                    &memories,
                    &meta,
                    self.current_focus.as_deref(),
                ) {
                    self.status_message = format!("Graph generation error: {}", e);
                } else {
                    let total_nodes = self.graph.graph.node_count();
                    let total_edges = self.graph.graph.edge_count();
                    let focus_str = match &self.current_focus {
                        Some(f) => format!(" [Focused: {}]", f),
                        None => " [Overview]".to_string(),
                    };
                    self.status_message = format!(
                        "Connected: {}{} ({} nodes, {} edges)",
                        db.db_path.display(),
                        focus_str,
                        total_nodes,
                        total_edges
                    );
                }

                self.memories = memories;
                self.meta = meta;
                self.db = Some(db);
            }
            Err(e) => {
                self.status_message = format!("Database error at {}: {}", self.db_path.display(), e);
            }
        }
    }
}

impl eframe::App for CratemdVizApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = (now - self.last_frame_time).as_secs_f32().min(0.05);
        self.last_frame_time = now;

        // Top Control Panel
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("cratemd visualizer");
                ui.separator();

                // Navigation breadcrumb and back button
                if self.current_focus.is_some() || !self.history.is_empty() {
                    if ui.button("<- Back").clicked() {
                        self.navigate_back();
                    }
                    let mut breadcrumb_parts = vec!["Overview".to_string()];
                    for item in &self.history {
                        if item != "root" {
                            let clean = item
                                .trim_start_matches("crate:")
                                .trim_start_matches("dep:")
                                .trim_start_matches("mod:");
                            breadcrumb_parts.push(clean.to_string());
                        }
                    }
                    if let Some(ref cur) = self.current_focus {
                        let clean = cur
                            .trim_start_matches("crate:")
                            .trim_start_matches("dep:")
                            .trim_start_matches("mod:");
                        breadcrumb_parts.push(clean.to_string());
                    }
                    ui.label(egui::RichText::new(breadcrumb_parts.join(" > ")).strong().color(Color32::from_rgb(255, 215, 0)));
                    ui.separator();
                }

                ui.label("Search:");
                let text_edit = ui.text_edit_singleline(&mut self.query);
                if text_edit.changed() {
                    self.reload_data();
                }

                if ui.button("Clear").clicked() {
                    self.query.clear();
                    self.reload_data();
                }

                ui.separator();

                if ui.button("Reload").clicked() {
                    self.reload_data();
                }

                ui.checkbox(&mut self.physics_enabled, "Physics Simulation");

                if ui.button("Reset View").clicked() {
                    self.zoom = 1.0;
                    self.pan = Vec2::ZERO;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Nodes: {} | Edges: {}", self.graph.graph.node_count(), self.graph.graph.edge_count()));
                });
            });
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_message);
            });
        });

        // Right Inspector Panel for Markdown and details
        egui::SidePanel::right("inspector_panel")
            .resizable(true)
            .default_width(420.0)
            .min_width(280.0)
            .show(ctx, |ui| {
                ui.heading("Inspector");
                ui.separator();

                if let Some(idx) = self.selected_node {
                    if let Some(node) = self.graph.graph.node_weight(idx) {
                        ui.label(egui::RichText::new(&node.label).strong().size(18.0).color(node.kind.color()));
                        ui.label(format!("ID: {}", node.id));
                        ui.label(format!("Kind: {} | Category: {}", node.kind.label(), node.category));

                        let target_drill_id = if matches!(
                            node.kind,
                            crate::graph::NodeKind::Crate
                                | crate::graph::NodeKind::Dependency
                                | crate::graph::NodeKind::Module
                        ) {
                            Some(node.id.clone())
                        } else {
                            None
                        };

                        if let Some(drill_id) = target_drill_id {
                            let action_label = if node.kind == crate::graph::NodeKind::Module {
                                format!("Enter Module {} ->", node.label)
                            } else {
                                format!("Explore {} Details ->", node.label)
                            };
                            if ui.button(egui::RichText::new(action_label).strong()).clicked() {
                                self.drill_down(drill_id);
                                return;
                            }
                        }

                        ui.separator();

                        egui::ScrollArea::vertical().show(ui, |ui| {
                            CommonMarkViewer::new().show(ui, &mut self.markdown_cache, &node.details);
                        });
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(30.0);
                        ui.label("Select any node on the graph to inspect its architecture, public APIs, or memory notes.");
                    });
                }
            });

        // Central Graph View (OpenGL Canvas)
        egui::CentralPanel::default().show(ctx, |ui| {
            let canvas_rect = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(canvas_rect, egui::Sense::click_and_drag());
            let painter = ui.painter_at(canvas_rect);

            // Handle Pan
            if response.dragged_by(egui::PointerButton::Middle) || (response.dragged_by(egui::PointerButton::Secondary)) {
                self.pan += response.drag_delta();
            }

            // Handle Zoom
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll_delta != 0.0 {
                let zoom_factor = (scroll_delta * 0.002).exp();
                self.zoom = (self.zoom * zoom_factor).clamp(0.2, 3.5);
            }

            // Physics step
            if self.physics_enabled {
                self.graph.step_simulation(dt, canvas_rect);
                ctx.request_repaint();
            }

            // Transform helpers
            let current_zoom = self.zoom;
            let current_pan = self.pan;
            let to_screen = |pos: Pos2| -> Pos2 {
                let centered = pos - canvas_rect.center();
                canvas_rect.center() + (centered * current_zoom) + current_pan
            };

            let from_screen = |pos: Pos2| -> Pos2 {
                let centered = pos - canvas_rect.center() - current_pan;
                canvas_rect.center() + (centered / current_zoom)
            };

            // Background grid
            let bg_color = Color32::from_rgb(22, 24, 29);
            painter.rect_filled(canvas_rect, 0.0, bg_color);

            // Draw Edges
            for edge in self.graph.graph.raw_edges() {
                let u = edge.source();
                let v = edge.target();

                if let (Some(node_u), Some(node_v)) = (self.graph.graph.node_weight(u), self.graph.graph.node_weight(v)) {
                    let p1 = to_screen(node_u.pos);
                    let p2 = to_screen(node_v.pos);

                    let edge_stroke = Stroke::new(1.2 * self.zoom, Color32::from_rgb(60, 70, 85));
                    painter.line_segment([p1, p2], edge_stroke);

                    if self.zoom > 1.1 && !edge.weight.label.is_empty() {
                        let mid = Pos2::new((p1.x + p2.x) * 0.5, (p1.y + p2.y) * 0.5);
                        let edge_font = FontId::proportional(9.0 * self.zoom.clamp(0.8, 1.2));
                        painter.text(
                            mid,
                            egui::Align2::CENTER_CENTER,
                            &edge.weight.label,
                            edge_font,
                            Color32::from_rgb(140, 150, 165),
                        );
                    }
                }
            }

            // Mouse interaction with nodes
            let pointer_pos = response.hover_pos();
            let mut clicked_node = None;
            let mut double_clicked_node = None;
            let mut hover_cursor = false;

            // Draw Nodes
            let indices: Vec<NodeIndex> = self.graph.graph.node_indices().collect();
            for &idx in &indices {
                let node = &self.graph.graph[idx];
                let screen_pos = to_screen(node.pos);
                let radius = node.kind.radius() * self.zoom;

                let is_selected = self.selected_node == Some(idx);
                let mut is_hovered = false;

                if let Some(mouse) = pointer_pos {
                    if (mouse - screen_pos).length() <= radius + 4.0 {
                        is_hovered = true;
                        hover_cursor = true;

                        if response.double_clicked() {
                            double_clicked_node = Some(idx);
                        } else if response.clicked() {
                            clicked_node = Some(idx);
                        }

                        if response.drag_started() && response.dragged_by(egui::PointerButton::Primary) {
                            self.dragged_node = Some(idx);
                        }
                    }
                }

                // Node circle
                let base_color = node.kind.color();
                let fill_color = if is_selected {
                    Color32::WHITE
                } else if is_hovered {
                    Color32::from_rgb(
                        base_color.r().saturating_add(40),
                        base_color.g().saturating_add(40),
                        base_color.b().saturating_add(40),
                    )
                } else {
                    base_color
                };

                let outline_stroke = if is_selected {
                    Stroke::new(3.0 * self.zoom, Color32::from_rgb(255, 215, 0))
                } else if is_hovered {
                    Stroke::new(2.0 * self.zoom, Color32::WHITE)
                } else {
                    Stroke::new(1.5 * self.zoom, Color32::from_rgb(20, 20, 20))
                };

                painter.circle(screen_pos, radius, fill_color, outline_stroke);

                // Level-of-Detail (LOD) Label text:
                // Only render text label if zoomed in sufficiently, or if node is hovered / selected,
                // or if it's a top-level workspace/crate/dependency node to avoid visual crowding.
                let show_label = is_hovered
                    || is_selected
                    || self.zoom >= 0.75
                    || matches!(node.kind, crate::graph::NodeKind::Workspace | crate::graph::NodeKind::Crate | crate::graph::NodeKind::Dependency);

                if show_label {
                    let text_color = if is_hovered || is_selected {
                        Color32::from_rgb(255, 255, 255)
                    } else if self.zoom < 0.85 {
                        Color32::from_rgb(160, 170, 185)
                    } else {
                        Color32::from_rgb(220, 225, 235)
                    };

                    let font_id = FontId::proportional(11.0 * self.zoom.clamp(0.7, 1.4));
                    let text_pos = screen_pos + Vec2::new(0.0, radius + 4.0 * self.zoom);
                    painter.text(
                        text_pos,
                        egui::Align2::CENTER_TOP,
                        &node.label,
                        font_id,
                        text_color,
                    );
                }
            }

            if hover_cursor {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            if let Some(idx) = double_clicked_node {
                if let Some(node) = self.graph.graph.node_weight(idx) {
                    let target_id = node.id.clone();
                    // Drill down into Crate, Dependency, or Module
                    if matches!(
                        node.kind,
                        crate::graph::NodeKind::Crate
                            | crate::graph::NodeKind::Dependency
                            | crate::graph::NodeKind::Module
                    ) {
                        self.drill_down(target_id);
                    } else {
                        self.selected_node = Some(idx);
                    }
                }
            } else if let Some(idx) = clicked_node {
                self.selected_node = Some(idx);
            }

            // Dragged node handling
            if let Some(drag_idx) = self.dragged_node {
                if response.dragged_by(egui::PointerButton::Primary) {
                    if let Some(mouse) = pointer_pos {
                        let sim_pos = from_screen(mouse);
                        if let Some(node) = self.graph.graph.node_weight_mut(drag_idx) {
                            node.pos = sim_pos;
                            node.vel = Vec2::ZERO;
                            node.pinned = true;
                        }
                    }
                } else {
                    if let Some(node) = self.graph.graph.node_weight_mut(drag_idx) {
                        node.pinned = false;
                    }
                    self.dragged_node = None;
                }
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let initial_path = args.get(1).map(PathBuf::from);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("cratemd visualizer")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "cratemd visualizer",
        native_options,
        Box::new(move |cc| Ok(Box::new(CratemdVizApp::new(cc, initial_path)))),
    )
}
