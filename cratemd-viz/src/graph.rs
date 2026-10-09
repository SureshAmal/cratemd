use egui::{Color32, Pos2, Vec2};
use petgraph::graph::{NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Workspace,
    Crate,
    Dependency,
    Module,
    Struct,
    Enum,
    Trait,
    Function,
    Symbol,
    Memory,
}

impl NodeKind {
    pub fn color(&self) -> Color32 {
        match self {
            NodeKind::Workspace => Color32::from_rgb(80, 160, 255),  // Soft Blue
            NodeKind::Crate => Color32::from_rgb(70, 200, 150),     // Emerald Green
            NodeKind::Dependency => Color32::from_rgb(60, 210, 210), // Cyan / Teal
            NodeKind::Module => Color32::from_rgb(235, 175, 75),    // Amber / Gold
            NodeKind::Struct => Color32::from_rgb(240, 110, 110),   // Coral Red
            NodeKind::Enum => Color32::from_rgb(245, 140, 80),      // Orange
            NodeKind::Trait => Color32::from_rgb(180, 130, 235),    // Purple
            NodeKind::Function => Color32::from_rgb(100, 190, 230), // Sky Blue
            NodeKind::Symbol => Color32::from_rgb(200, 200, 200),   // Neutral Gray
            NodeKind::Memory => Color32::from_rgb(175, 120, 240),   // Lavender Purple
        }
    }

    pub fn radius(&self) -> f32 {
        match self {
            NodeKind::Workspace => 22.0,
            NodeKind::Crate => 18.0,
            NodeKind::Dependency => 16.0,
            NodeKind::Module => 14.0,
            NodeKind::Struct => 11.0,
            NodeKind::Enum => 11.0,
            NodeKind::Trait => 11.0,
            NodeKind::Function => 9.0,
            NodeKind::Symbol => 10.0,
            NodeKind::Memory => 16.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            NodeKind::Workspace => "Workspace",
            NodeKind::Crate => "Crate",
            NodeKind::Dependency => "Dependency",
            NodeKind::Module => "Module",
            NodeKind::Struct => "Struct",
            NodeKind::Enum => "Enum",
            NodeKind::Trait => "Trait",
            NodeKind::Function => "Function",
            NodeKind::Symbol => "Symbol",
            NodeKind::Memory => "Memory",
        }
    }
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: NodeKind,
    pub details: String,
    pub category: String,

    // Physics state
    pub pos: Pos2,
    pub vel: Vec2,
    pub pinned: bool,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub label: String,
    pub weight: f32,
}

pub struct VizGraph {
    pub graph: UnGraph<GraphNode, GraphEdge>,
    pub id_map: HashMap<String, NodeIndex>,
}

impl VizGraph {
    pub fn new() -> Self {
        Self {
            graph: UnGraph::new_undirected(),
            id_map: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.graph.clear();
        self.id_map.clear();
    }

    pub fn add_or_get_node(
        &mut self,
        id: &str,
        label: &str,
        kind: NodeKind,
        details: &str,
        category: &str,
        initial_pos: Pos2,
    ) -> NodeIndex {
        if let Some(&idx) = self.id_map.get(id) {
            return idx;
        }

        let node = GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            kind,
            details: details.to_string(),
            category: category.to_string(),
            pos: initial_pos,
            vel: Vec2::ZERO,
            pinned: false,
        };

        let idx = self.graph.add_node(node);
        self.id_map.insert(id.to_string(), idx);
        idx
    }

    pub fn add_edge(&mut self, a: NodeIndex, b: NodeIndex, label: &str, weight: f32) {
        if a != b {
            self.graph.add_edge(a, b, GraphEdge {
                label: label.to_string(),
                weight,
            });
        }
    }

    /// Step the 2D force-directed physics simulation
    pub fn step_simulation(&mut self, dt: f32, bounds: egui::Rect) {
        let node_count = self.graph.node_count();
        if node_count == 0 {
            return;
        }

        let indices: Vec<NodeIndex> = self.graph.node_indices().collect();
        let mut forces: HashMap<NodeIndex, Vec2> = HashMap::with_capacity(node_count);

        for &idx in &indices {
            forces.insert(idx, Vec2::ZERO);
        }

        // 1. Repulsion between all node pairs (Coulomb-style repulsion)
        let repulsion_k = 12000.0;
        let min_dist = 20.0;

        for i in 0..indices.len() {
            let u = indices[i];
            let pos_u = self.graph[u].pos;

            for j in (i + 1)..indices.len() {
                let v = indices[j];
                let pos_v = self.graph[v].pos;

                let delta = pos_u - pos_v;
                let mut dist = delta.length();
                if dist < min_dist {
                    dist = min_dist;
                }

                let dir = if dist > 0.001 { delta / dist } else { Vec2::new(1.0, 0.0) };
                let force_mag = repulsion_k / (dist * dist);
                let force = dir * force_mag;

                if let Some(f) = forces.get_mut(&u) {
                    *f += force;
                }
                if let Some(f) = forces.get_mut(&v) {
                    *f -= force;
                }
            }
        }

        // 2. Spring attraction along edges (Hooke's law)
        let spring_k = 0.04;
        let rest_length = 90.0;

        for edge in self.graph.edge_references() {
            let u = edge.source();
            let v = edge.target();

            let pos_u = self.graph[u].pos;
            let pos_v = self.graph[v].pos;

            let delta = pos_v - pos_u;
            let dist = delta.length();
            let displacement = dist - rest_length;

            let dir = if dist > 0.001 { delta / dist } else { Vec2::ZERO };
            let force = dir * (displacement * spring_k * edge.weight().weight);

            if let Some(f) = forces.get_mut(&u) {
                *f += force;
            }
            if let Some(f) = forces.get_mut(&v) {
                *f -= force;
            }
        }

        // 3. Center gravity towards bounds center
        let center = bounds.center();
        let gravity_k = 0.015;

        for &idx in &indices {
            let pos = self.graph[idx].pos;
            let delta = center - pos;
            if let Some(f) = forces.get_mut(&idx) {
                *f += delta * gravity_k;
            }
        }

        // 4. Update velocity and positions with damping
        let damping = 0.82;
        let max_speed = 30.0;

        for &idx in &indices {
            let node = &mut self.graph[idx];
            if node.pinned {
                node.vel = Vec2::ZERO;
                continue;
            }

            if let Some(&f) = forces.get(&idx) {
                node.vel = (node.vel + f * dt) * damping;

                // Clamp velocity
                let speed = node.vel.length();
                if speed > max_speed {
                    node.vel = (node.vel / speed) * max_speed;
                }

                node.pos += node.vel;

                // Constrain within bounds margin
                let margin = 35.0;
                node.pos.x = node.pos.x.clamp(bounds.min.x + margin, bounds.max.x - margin);
                node.pos.y = node.pos.y.clamp(bounds.min.y + margin, bounds.max.y - margin);
            }
        }
    }
}
