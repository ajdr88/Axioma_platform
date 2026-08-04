/**
 * Fixed footprint every `AxiomaBlockNode` renders today — every node always renders
 * `properties: []` (real properties live in the Postgres body, not on the node), so the visible
 * card size is uniform in practice: header row (dot + label + badges) + an always-empty
 * "Properties" label row. Used both as the actual React Flow node `width`/`height` (so the
 * card's real footprint matches this exactly, with the label truncating instead of overflowing)
 * and as the node size ELK lays out around — the two must stay equal, or ELK's overlap-avoidance
 * is computed against a box that doesn't match what's actually on screen.
 */
export const NODE_WIDTH = 220;
export const NODE_HEIGHT = 92;
