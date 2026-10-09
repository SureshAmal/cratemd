use crate::graph::{NodeKind, VizGraph};
use egui::Pos2;

#[test]
fn test_viz_graph_add_nodes_and_edges() {
    let mut graph = VizGraph::new();
    let n1 = graph.add_or_get_node(
        "root",
        "cratemd",
        NodeKind::Workspace,
        "# cratemd workspace",
        "workspace",
        Pos2::new(100.0, 100.0),
    );
    let n2 = graph.add_or_get_node(
        "crate:cratemd-viz",
        "cratemd-viz",
        NodeKind::Crate,
        "# cratemd-viz crate",
        "crates",
        Pos2::new(200.0, 200.0),
    );

    graph.add_edge(n1, n2, "member", 1.0);

    assert_eq!(graph.graph.node_count(), 2);
    assert_eq!(graph.graph.edge_count(), 1);

    // Verify retrieving existing node returns identical index
    let n1_again = graph.add_or_get_node(
        "root",
        "cratemd",
        NodeKind::Workspace,
        "# cratemd workspace",
        "workspace",
        Pos2::new(100.0, 100.0),
    );
    assert_eq!(n1, n1_again);
}

#[test]
fn test_viz_graph_simulation_step() {
    let mut graph = VizGraph::new();
    let n1 = graph.add_or_get_node(
        "a",
        "A",
        NodeKind::Module,
        "detail A",
        "cat",
        Pos2::new(100.0, 100.0),
    );
    let n2 = graph.add_or_get_node(
        "b",
        "B",
        NodeKind::Module,
        "detail B",
        "cat",
        Pos2::new(105.0, 105.0),
    );
    graph.add_edge(n1, n2, "connected", 1.0);

    let bounds = egui::Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(800.0, 600.0));
    let initial_pos_a = graph.graph[n1].pos;

    // Run 5 simulation steps
    for _ in 0..5 {
        graph.step_simulation(0.016, bounds);
    }

    let final_pos_a = graph.graph[n1].pos;
    // Nodes should repel or adjust their coordinates
    assert_ne!(initial_pos_a, final_pos_a);
}
