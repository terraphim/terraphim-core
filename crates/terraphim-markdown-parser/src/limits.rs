//! Configurable resource limits for markdown parsing and extraction.
//!
//! All limits have secure defaults. Callers may lower any limit freely;
//! raising a limit above the secure default requires the explicit `with_*`
//! builders so accidental unbounded parsing cannot happen silently.

/// Which resource limit was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitKind {
    /// Total input size in bytes.
    InputBytes,
    /// Maximum nesting depth of the AST.
    NestingDepth,
    /// Maximum number of AST nodes.
    NodeCount,
    /// Maximum length of a single line in bytes.
    LineLength,
    /// Maximum size of the normalized output in bytes.
    OutputBytes,
}

impl core::fmt::Display for LimitKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let name = match self {
            LimitKind::InputBytes => "input bytes",
            LimitKind::NestingDepth => "nesting depth",
            LimitKind::NodeCount => "node count",
            LimitKind::LineLength => "line length",
            LimitKind::OutputBytes => "output bytes",
        };
        f.write_str(name)
    }
}

/// Error produced while validating pre-parse limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseLimitsError {
    #[error("input exceeds limit of {limit} bytes (actual: {actual})")]
    InputTooLarge { limit: usize, actual: usize },
    #[error("line exceeds limit of {limit} bytes (actual: {actual})")]
    LineTooLong { limit: usize, actual: usize },
}

/// Configurable parsing limits with secure defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_input_bytes: usize,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_line_length: usize,
    pub max_output_bytes: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 4 * 1024 * 1024,
            max_depth: 128,
            max_nodes: 100_000,
            max_line_length: 8 * 1024,
            max_output_bytes: 16 * 1024 * 1024,
        }
    }
}

impl ParseLimits {
    /// Explicitly raise or set the input byte limit.
    pub fn with_max_input_bytes(mut self, value: usize) -> Self {
        self.max_input_bytes = value;
        self
    }

    /// Explicitly raise or set the nesting depth limit.
    pub fn with_max_depth(mut self, value: usize) -> Self {
        self.max_depth = value;
        self
    }

    /// Explicitly raise or set the node count limit.
    pub fn with_max_nodes(mut self, value: usize) -> Self {
        self.max_nodes = value;
        self
    }

    /// Explicitly raise or set the line length limit.
    pub fn with_max_line_length(mut self, value: usize) -> Self {
        self.max_line_length = value;
        self
    }

    /// Explicitly raise or set the output byte limit.
    pub fn with_max_output_bytes(mut self, value: usize) -> Self {
        self.max_output_bytes = value;
        self
    }

    /// Lower the input byte limit. Never raises it.
    pub fn clamp_max_input_bytes(mut self, value: usize) -> Self {
        self.max_input_bytes = self.max_input_bytes.min(value);
        self
    }

    /// Lower the depth limit. Never raises it.
    pub fn clamp_max_depth(mut self, value: usize) -> Self {
        self.max_depth = self.max_depth.min(value);
        self
    }

    /// Lower the node count limit. Never raises it.
    pub fn clamp_max_nodes(mut self, value: usize) -> Self {
        self.max_nodes = self.max_nodes.min(value);
        self
    }

    /// Lower the line length limit. Never raises it.
    pub fn clamp_max_line_length(mut self, value: usize) -> Self {
        self.max_line_length = self.max_line_length.min(value);
        self
    }

    /// Lower the output byte limit. Never raises it.
    pub fn clamp_max_output_bytes(mut self, value: usize) -> Self {
        self.max_output_bytes = self.max_output_bytes.min(value);
        self
    }

    /// Validate limits that can be checked before parsing begins.
    pub fn validate_input(&self, markdown: &str) -> Result<(), ParseLimitsError> {
        if markdown.len() > self.max_input_bytes {
            return Err(ParseLimitsError::InputTooLarge {
                limit: self.max_input_bytes,
                actual: markdown.len(),
            });
        }
        let longest = markdown
            .split('\n')
            .map(|line| line.len())
            .max()
            .unwrap_or(0);
        if longest > self.max_line_length {
            return Err(ParseLimitsError::LineTooLong {
                limit: self.max_line_length,
                actual: longest,
            });
        }
        Ok(())
    }
}

/// A single AST node/depth measurement result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AstStats {
    pub nodes: usize,
    pub depth: usize,
}

/// Compute node count and maximum nesting depth with checked arithmetic.
pub fn measure_ast(root: &markdown::mdast::Node) -> Option<AstStats> {
    fn walk(node: &markdown::mdast::Node, depth: usize, stats: &mut AstStats) -> Option<()> {
        stats.nodes = stats.nodes.checked_add(1)?;
        stats.depth = stats.depth.max(depth.checked_add(1)?);
        for child in ast_children(node).into_iter().flatten() {
            walk(child, depth.checked_add(1)?, stats)?;
        }
        Some(())
    }
    let mut stats = AstStats { nodes: 0, depth: 0 };
    walk(root, 0, &mut stats)?;
    Some(stats)
}

/// Enumerate the direct children of a node, if any.
pub(crate) fn ast_children(node: &markdown::mdast::Node) -> Option<&Vec<markdown::mdast::Node>> {
    use markdown::mdast::Node;
    match node {
        Node::Root(n) => Some(&n.children),
        Node::Blockquote(n) => Some(&n.children),
        Node::List(n) => Some(&n.children),
        Node::ListItem(n) => Some(&n.children),
        Node::Paragraph(n) => Some(&n.children),
        Node::Heading(n) => Some(&n.children),
        Node::Table(n) => Some(&n.children),
        Node::TableRow(n) => Some(&n.children),
        Node::TableCell(n) => Some(&n.children),
        Node::Emphasis(n) => Some(&n.children),
        Node::Strong(n) => Some(&n.children),
        Node::Delete(n) => Some(&n.children),
        Node::Link(n) => Some(&n.children),
        Node::LinkReference(n) => Some(&n.children),
        Node::ImageReference(_) => None,
        Node::FootnoteDefinition(n) => Some(&n.children),
        Node::FootnoteReference(_) => None,
        Node::Toml(_) => None,
        Node::Yaml(_) => None,
        Node::Break(_) => None,
        Node::Code(_) => None,
        Node::Definition(_) => None,
        Node::Html(_) => None,
        Node::Image(_) => None,
        Node::InlineCode(_) => None,
        Node::InlineMath(_) => None,
        Node::MdxJsxFlowElement(_) => None,
        Node::MdxjsEsm(_) => None,
        Node::MdxTextExpression(_) => None,
        Node::MdxFlowExpression(_) => None,
        Node::MdxJsxTextElement(_) => None,
        Node::Math(_) => None,
        Node::Text(_) => None,
        Node::ThematicBreak(_) => None,
    }
}
