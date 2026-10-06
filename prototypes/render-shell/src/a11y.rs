//! Spike: what accessibility costs without a toolkit. AccessKit exposes a
//! hand-built tree (transport buttons, track toggles) to UI Automation. A
//! real custom toolkit would generate this from its widget tree and keep it
//! updated, with focus, bounds and actions.

use accesskit::{
    Action, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node, NodeId,
    Role, Tree, TreeId, TreeUpdate,
};

pub struct Activation;
pub struct Actions;
pub struct Deactivation;

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(tree())
    }
}

impl ActionHandler for Actions {
    fn do_action(&mut self, _request: ActionRequest) {}
}

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

fn tree() -> TreeUpdate {
    let root = NodeId(0);
    let mut nodes = Vec::new();
    let mut children = Vec::new();
    let mut add = |id: u64, role: Role, label: String| {
        let mut node = Node::new(role);
        node.set_label(label);
        node.add_action(Action::Click);
        children.push(NodeId(id));
        nodes.push((NodeId(id), node));
    };
    for (i, label) in ["Play", "Stop", "Record", "Loop"].into_iter().enumerate() {
        add(1 + i as u64, Role::Button, label.to_string());
    }
    for track in 1..=8_u64 {
        add(100 + track, Role::CheckBox, format!("Mute Track {track}"));
        add(200 + track, Role::CheckBox, format!("Solo Track {track}"));
    }
    let mut window = Node::new(Role::Window);
    window.set_label("Lumen render shell");
    window.set_children(children);
    nodes.insert(0, (root, window));
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(root)),
        tree_id: TreeId::ROOT,
        focus: root,
    }
}
