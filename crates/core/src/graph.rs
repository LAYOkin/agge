use crate::{Edge, Index};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    In,
    Out,
    Both,
}

#[derive(Debug, Default)]
pub(crate) struct Graph {
    nodes: BTreeMap<String, Links>,
}

#[derive(Debug, Default)]
struct Links {
    incoming: Vec<usize>,
    outgoing: Vec<usize>,
}

impl Graph {
    fn new(edges: &[Edge]) -> Self {
        let mut graph = Self::default();
        for (i, edge) in edges.iter().enumerate() {
            graph
                .nodes
                .entry(edge.to.clone())
                .or_default()
                .incoming
                .push(i);
            graph
                .nodes
                .entry(edge.from.clone())
                .or_default()
                .outgoing
                .push(i);
        }
        graph
    }
}

impl Index {
    fn adjacency(&self) -> &Graph {
        self.graph.get_or_init(|| Graph::new(&self.edges))
    }

    pub fn has_node(&self, key: &str) -> bool {
        self.symbols.contains_key(key) || self.adjacency().nodes.contains_key(key)
    }

    pub fn references(&self, key: &str, direction: Direction) -> Vec<&Edge> {
        let Some(links) = self.adjacency().nodes.get(key) else {
            return Vec::new();
        };
        match direction {
            Direction::In => links.incoming.iter().map(|&i| &self.edges[i]).collect(),
            Direction::Out => links.outgoing.iter().map(|&i| &self.edges[i]).collect(),
            Direction::Both => {
                let mut indices: Vec<_> = links
                    .incoming
                    .iter()
                    .chain(&links.outgoing)
                    .copied()
                    .collect();
                indices.sort_unstable();
                indices.dedup();
                indices.into_iter().map(|i| &self.edges[i]).collect()
            }
        }
    }

    /// Stable breadth-first traversal returning minimum distance from any root.
    pub fn walk<'a>(
        &'a self,
        roots: &'a [String],
        direction: Direction,
        depth: u8,
    ) -> Vec<(&'a str, u8)> {
        let graph = self.adjacency();
        let mut level: BTreeSet<_> = roots.iter().map(String::as_str).collect();
        let mut seen = level.clone();
        let mut result = Vec::new();
        for distance in 1..=depth {
            let mut next = BTreeSet::new();
            for node in level {
                let Some(links) = graph.nodes.get(node) else {
                    continue;
                };
                let (incoming, outgoing) = match direction {
                    Direction::In => (links.incoming.as_slice(), &[][..]),
                    Direction::Out => (&[][..], links.outgoing.as_slice()),
                    Direction::Both => (links.incoming.as_slice(), links.outgoing.as_slice()),
                };
                for (links, target) in [(incoming, true), (outgoing, false)] {
                    for &i in links {
                        let edge = &self.edges[i];
                        let key = if target { &edge.from } else { &edge.to };
                        if seen.insert(key.as_str()) {
                            next.insert(key.as_str());
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            result.extend(next.iter().map(|&key| (key, distance)));
            level = next;
        }
        result
    }
}
