//! dagre's default ranker, network simplex (`lib/rank/network-simplex.js` and
//! `feasible-tree.js`), over the graph dagre ranks: each link stretched to twice its
//! length, as `makeSpaceForEdgeLabels` doubles `minlen`, and a root node with a link of
//! weight 0 and length 1 to every node, as `nestingGraph.run` adds one. The root costs
//! nothing, but it connects the graph and takes part in breaking ties, so it is kept.
//!
//! Ties are broken as dagre breaks them, which depends on the order graphlib lists nodes
//! and edges in: nodes with an array-index id first in ascending order, then the others in
//! the order they were added; edges in the order they were added.

use std::collections::HashMap;

/// The order a link imposes on the layers of two nodes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Constraint {
    /// The node the link leaves.
    pub from: usize,
    /// The node the link enters, at least `length` layers after `from`.
    pub to: usize,
    /// The link's length in layers.
    pub length: usize,
}

/// One edge of the ranked graph, all links between the same two nodes merged into it as
/// dagre's `simplify` merges them.
struct RankEdge {
    from: usize,
    to: usize,
    weight: i64,
    minlen: i64,
}

/// Bounds the tree changes the search makes. dagre changes the tree until no cut value is
/// negative, but network simplex can cycle through trees of equal cost, and a terminal
/// drawing must finish; the ranks are valid after every change, so the search stops there.
/// A chart that reaches the bound keeps the ranks of the last change, which may span more
/// layers than upstream's; no chart that reaches it is known.
const MAX_EXCHANGES: usize = 10_000;

/// The layer of each node, indexed like `array_index`, which gives each node's id as an
/// array index when it is one: network simplex ranks the nodes so that every constraint's
/// `to` lies at least its `length` layers after its `from`, with the fewest layers spanned
/// by all constraints together, ties broken as dagre breaks them. `constraints` come in the
/// order dagre's graph holds its edges, and a constraint from a node to itself is ignored.
/// `None` when the constraints form a cycle or name a node outside `array_index`.
pub(super) fn network_simplex_layers(
    array_index: &[Option<u32>],
    constraints: &[Constraint],
) -> Option<Vec<usize>> {
    let mut ranker = Ranker::new(array_index, constraints)?;
    ranker.run()?;
    let ranks = ranker.rank.get(..array_index.len())?;
    let Some(&first) = ranks.iter().min() else { return Some(Vec::new()) };
    // Every rank has the parity of the others, as each tree link stretches over an even
    // span or, through the root, over one each way.
    ranks.iter().map(|&rank| usize::try_from((rank - first) / 2).ok()).collect()
}

/// The ranked graph and the tree network simplex keeps over it.
struct Ranker {
    edges: Vec<RankEdge>,
    /// The edges into each node, then those out of it, as graphlib's `nodeEdges` lists them.
    incident: Vec<Vec<usize>>,
    /// The order graphlib lists nodes in, as a sort key.
    key: Vec<(bool, Option<u32>)>,
    rank: Vec<i64>,
    /// Each node in the tree, in the order it joined.
    tree_nodes: Vec<usize>,
    /// The tree's nodes in the order graphlib lists them.
    listed: Vec<usize>,
    in_tree: Vec<bool>,
    /// Each tree edge, in the order it joined.
    tree_edges: Vec<usize>,
    is_tree_edge: Vec<bool>,
    /// The cut value of each tree edge.
    cut: Vec<i64>,
    low: Vec<usize>,
    lim: Vec<usize>,
    /// The tree edge to each node's parent, `None` for the tree's root.
    parent_edge: Vec<Option<usize>>,
    /// The nodes the last numbering reached, each after its parent.
    preorder: Vec<usize>,
    /// Each node on a depth-first search's path, with the next of its edges to follow.
    path: Vec<(usize, usize)>,
    /// The first node graphlib lists. It starts the tree, and roots it once the tree spans
    /// every node.
    start: usize,
}

impl Ranker {
    fn new(array_index: &[Option<u32>], constraints: &[Constraint]) -> Option<Ranker> {
        let count = array_index.len();
        let root = count;
        let mut edges: Vec<RankEdge> = Vec::new();
        let mut index_of: HashMap<(usize, usize), usize> = HashMap::new();
        for constraint in constraints.iter().filter(|c| c.from != c.to) {
            if constraint.from >= count || constraint.to >= count {
                return None;
            }
            let minlen = i64::try_from(constraint.length).ok()?.checked_mul(2)?;
            let index = *index_of.entry((constraint.from, constraint.to)).or_insert_with(|| {
                edges.push(RankEdge {
                    from: constraint.from,
                    to: constraint.to,
                    weight: 0,
                    minlen: 1,
                });
                edges.len() - 1
            });
            let edge = edges.get_mut(index)?;
            edge.weight += 1;
            edge.minlen = edge.minlen.max(minlen);
        }
        let mut key: Vec<(bool, Option<u32>)> =
            array_index.iter().map(|&index| (index.is_none(), index)).collect();
        key.push((true, None));
        let mut nodes: Vec<usize> = (0..count).collect();
        nodes.sort_by_key(|&node| key.get(node).copied());
        for &node in &nodes {
            edges.push(RankEdge { from: root, to: node, weight: 0, minlen: 1 });
        }
        let mut entering = vec![Vec::new(); count + 1];
        let mut leaving = vec![Vec::new(); count + 1];
        for (index, edge) in edges.iter().enumerate() {
            entering.get_mut(edge.to)?.push(index);
            leaving.get_mut(edge.from)?.push(index);
        }
        let incident = entering.into_iter().zip(leaving).map(|(into, out)| [into, out].concat());
        let rank = longest_path(&edges, count + 1)?;
        // graphlib lists the root, added last and without an array-index id, last.
        let start = nodes.first().copied().unwrap_or(root);
        let edge_count = edges.len();
        let mut in_tree = vec![false; count + 1];
        *in_tree.get_mut(start)? = true;
        Some(Ranker {
            edges,
            incident: incident.collect(),
            key,
            rank,
            tree_nodes: vec![start],
            listed: vec![start],
            in_tree,
            tree_edges: Vec::new(),
            is_tree_edge: vec![false; edge_count],
            cut: vec![0; edge_count],
            low: vec![0; count + 1],
            lim: vec![0; count + 1],
            parent_edge: vec![None; count + 1],
            preorder: Vec::new(),
            path: Vec::new(),
            start,
        })
    }

    /// dagre's `networkSimplex` after `longestPath`.
    fn run(&mut self) -> Option<()> {
        self.feasible_tree()?;
        self.number_below(self.start, 1)?;
        for at in (0..self.preorder.len()).rev() {
            self.update_cut(*self.preorder.get(at)?)?;
        }
        for _ in 0..MAX_EXCHANGES {
            let Some(leaving) = self
                .tree_edges
                .iter()
                .copied()
                .find(|&edge| self.cut.get(edge).is_some_and(|&cut| cut < 0))
            else {
                break;
            };
            let entering = self.enter_edge(leaving)?;
            self.exchange(leaving, entering)?;
        }
        Some(())
    }

    fn slack(&self, edge: &RankEdge) -> Option<i64> {
        Some(self.rank.get(edge.to)? - self.rank.get(edge.from)? - edge.minlen)
    }

    /// dagre's `feasibleTree`: grows a tree of tight edges from the first node, shifting
    /// the tree's ranks to tighten the edge with the least slack leaving it whenever no
    /// tight edge does.
    fn feasible_tree(&mut self) -> Option<()> {
        while self.tight_tree()? < self.rank.len() {
            let mut least: Option<(i64, usize)> = None;
            for (index, edge) in self.edges.iter().enumerate() {
                if self.in_tree.get(edge.from)? != self.in_tree.get(edge.to)? {
                    let slack = self.slack(edge)?;
                    if least.is_none_or(|(fewest, _)| slack < fewest) {
                        least = Some((slack, index));
                    }
                }
            }
            let (slack, index) = least?;
            let delta =
                if *self.in_tree.get(self.edges.get(index)?.from)? { slack } else { -slack };
            for &node in &self.tree_nodes {
                *self.rank.get_mut(node)? += delta;
            }
        }
        Some(())
    }

    /// dagre's `tightTree`: from each tree node in turn, a depth-first search adds every
    /// node reached by a tight edge. Returns the tree's size.
    fn tight_tree(&mut self) -> Option<usize> {
        let joined = self.tree_nodes.len();
        // graphlib lists the tree's nodes as they were when the search began.
        for at in 0..self.listed.len() {
            self.path.clear();
            self.path.push((*self.listed.get(at)?, 0));
            while let Some(last) = self.path.last_mut() {
                let (node, next) = *last;
                last.1 += 1;
                let Some(&index) = self.incident.get(node)?.get(next) else {
                    self.path.pop();
                    continue;
                };
                let edge = self.edges.get(index)?;
                let other = if edge.from == node { edge.to } else { edge.from };
                if !*self.in_tree.get(other)? && self.slack(edge)? == 0 {
                    *self.in_tree.get_mut(other)? = true;
                    self.tree_nodes.push(other);
                    self.tree_edges.push(index);
                    *self.is_tree_edge.get_mut(index)? = true;
                    self.path.push((other, 0));
                }
            }
        }
        // A stable sort keeps nodes of equal key in the order they joined, as graphlib
        // lists them. With the joined nodes sorted first, it merges two sorted runs.
        let key = &self.key;
        let sorted = self.listed.len();
        self.listed.extend_from_slice(self.tree_nodes.get(joined..)?);
        self.listed.get_mut(sorted..)?.sort_by_key(|&node| key.get(node).copied());
        self.listed.sort_by_key(|&node| key.get(node).copied());
        Some(self.tree_nodes.len())
    }

    /// dagre's `exchangeEdges`: replaces `leaving` with `entering` in the tree, then renews
    /// the numbers, cut values and ranks. dagre renews them over the whole tree, but only
    /// the subtree of the deepest node above both edges changes shape, so only it is
    /// numbered again; it keeps its size, so its numbers keep their range. Only the tree
    /// edges on the cycle `entering` closes split the nodes differently, so only their cut
    /// values are computed again.
    fn exchange(&mut self, leaving: usize, entering: usize) -> Option<()> {
        let edge = self.edges.get(leaving)?;
        // `leaving` cuts `child`'s subtree off `parent`.
        let (child, parent) = if self.lim.get(edge.from)? < self.lim.get(edge.to)? {
            (edge.from, edge.to)
        } else {
            (edge.to, edge.from)
        };
        let edge = self.edges.get(entering)?;
        let outside = if self.is_below(edge.from, child)? { edge.to } else { edge.from };
        let mut top = parent;
        while !self.is_below(outside, top)? {
            top = self.tree_parent(top)?;
        }
        self.tree_edges.retain(|&edge| edge != leaving);
        self.tree_edges.push(entering);
        *self.is_tree_edge.get_mut(leaving)? = false;
        *self.is_tree_edge.get_mut(entering)? = true;
        self.number_below(top, *self.low.get(top)?)?;
        // The cycle runs from `child` up the new tree through `entering` to `top`, and from
        // `parent` up to `top`. Each node's other tree edges below it keep their cut values.
        for mut node in [child, parent] {
            while node != top {
                self.update_cut(node)?;
                node = self.tree_parent(node)?;
            }
        }
        self.update_ranks()
    }

    /// Whether `node` lies in the subtree of `top`, dagre's `isDescendant`.
    fn is_below(&self, node: usize, top: usize) -> Option<bool> {
        let at = *self.lim.get(node)?;
        Some(*self.low.get(top)? <= at && at <= *self.lim.get(top)?)
    }

    /// The node above `node` in the tree, `None` for the tree's root.
    fn tree_parent(&self, node: usize) -> Option<usize> {
        let edge = self.edges.get((*self.parent_edge.get(node)?)?)?;
        Some(if edge.from == node { edge.to } else { edge.from })
    }

    /// dagre's `dfsAssignLowLim` over the subtree of `top`, numbered from `first`, in a tree
    /// rooted at `start`, its root once `feasible_tree` has run. Which neighbor the search
    /// visits first changes the numbers but not what they say: which nodes lie below an
    /// edge, and so each tree edge's cut value.
    fn number_below(&mut self, top: usize, first: usize) -> Option<()> {
        let mut next_lim = first;
        self.preorder.clear();
        self.preorder.push(top);
        *self.low.get_mut(top)? = next_lim;
        self.path.clear();
        self.path.push((top, 0));
        while let Some(last) = self.path.last_mut() {
            let (node, next) = *last;
            last.1 += 1;
            let Some(&index) = self.incident.get(node)?.get(next) else {
                *self.lim.get_mut(node)? = next_lim;
                next_lim += 1;
                self.path.pop();
                continue;
            };
            if !*self.is_tree_edge.get(index)? || *self.parent_edge.get(node)? == Some(index) {
                continue;
            }
            let edge = self.edges.get(index)?;
            let below = if edge.from == node { edge.to } else { edge.from };
            *self.low.get_mut(below)? = next_lim;
            *self.parent_edge.get_mut(below)? = Some(index);
            self.preorder.push(below);
            self.path.push((below, 0));
        }
        Some(())
    }

    /// dagre's `assignCutValue` for the tree edge from `child` to its parent, if any.
    fn update_cut(&mut self, child: usize) -> Option<()> {
        if let Some(index) = *self.parent_edge.get(child)? {
            let cut = self.cut_value(child, index)?;
            *self.cut.get_mut(index)? = cut;
        }
        Some(())
    }

    /// dagre's `calcCutValue` for the tree edge `parent_edge` from `child` to its parent,
    /// once the cut values of the tree edges below `child` are known.
    fn cut_value(&self, child: usize, parent_edge: usize) -> Option<i64> {
        let tree_edge = self.edges.get(parent_edge)?;
        let child_is_tail = tree_edge.from == child;
        let parent = if child_is_tail { tree_edge.to } else { tree_edge.from };
        let mut cut = tree_edge.weight;
        for &index in self.incident.get(child)? {
            let edge = self.edges.get(index)?;
            let is_out = edge.from == child;
            let other = if is_out { edge.to } else { edge.from };
            if other == parent {
                continue;
            }
            let points_to_head = is_out == child_is_tail;
            cut += if points_to_head { edge.weight } else { -edge.weight };
            if *self.is_tree_edge.get(index)? {
                let below = *self.cut.get(index)?;
                cut += if points_to_head { -below } else { below };
            }
        }
        Some(cut)
    }

    /// dagre's `enterEdge`: the edge with the least slack, the first among equals, from
    /// the head's side of `leaving` to its tail's side.
    fn enter_edge(&self, leaving: usize) -> Option<usize> {
        let edge = self.edges.get(leaving)?;
        let (tail, flip) = if self.lim.get(edge.from)? > self.lim.get(edge.to)? {
            (edge.to, true)
        } else {
            (edge.from, false)
        };
        let (low, lim) = (*self.low.get(tail)?, *self.lim.get(tail)?);
        let below = |node: usize| self.lim.get(node).map(|&at| low <= at && at <= lim);
        let mut least: Option<(i64, usize)> = None;
        for (index, edge) in self.edges.iter().enumerate() {
            if flip == below(edge.from)? && flip != below(edge.to)? {
                let slack = self.slack(edge)?;
                if least.is_none_or(|(fewest, _)| slack < fewest) {
                    least = Some((slack, index));
                }
            }
        }
        least.map(|(_, index)| index)
    }

    /// dagre's `updateRanks`: each node at its tree edge's `minlen` from its parent. Only
    /// the nodes below the last numbering's top move, as every tree edge was tight before.
    fn update_ranks(&mut self) -> Option<()> {
        for &node in self.preorder.get(1..)? {
            let edge = self.edges.get((*self.parent_edge.get(node)?)?)?;
            let (parent, offset) =
                if edge.from == node { (edge.to, -edge.minlen) } else { (edge.from, edge.minlen) };
            let rank = self.rank.get(parent)? + offset;
            *self.rank.get_mut(node)? = rank;
        }
        Some(())
    }
}

/// dagre's `longestPath`: a node without outgoing edges takes rank 0, any other the
/// smallest `child rank - minlen` over its edges. `None` when the edges form a cycle.
fn longest_path(edges: &[RankEdge], count: usize) -> Option<Vec<i64>> {
    let mut unranked_children = vec![0_usize; count];
    let mut entering = vec![Vec::new(); count];
    for edge in edges {
        *unranked_children.get_mut(edge.from)? += 1;
        entering.get_mut(edge.to)?.push(edge);
    }
    let mut rank: Vec<Option<i64>> = vec![None; count];
    let mut ready: Vec<usize> =
        (0..count).filter(|&node| unranked_children.get(node) == Some(&0)).collect();
    let mut ranked = 0;
    while let Some(node) = ready.pop() {
        ranked += 1;
        let at = *rank.get_mut(node)?.get_or_insert(0);
        for edge in entering.get(node)? {
            let parent = rank.get_mut(edge.from)?;
            let candidate = at - edge.minlen;
            *parent = Some(parent.map_or(candidate, |rank| rank.min(candidate)));
            let unranked = unranked_children.get_mut(edge.from)?;
            *unranked -= 1;
            if *unranked == 0 {
                ready.push(edge.from);
            }
        }
    }
    (ranked == count).then(|| rank.into_iter().map(|rank| rank.unwrap_or(0)).collect())
}
