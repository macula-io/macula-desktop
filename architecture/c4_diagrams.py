"""Generate macula-desktop's two C4 diagrams into assets/.

Run:  python3 architecture/c4_diagrams.py   (from the repository root)

  c4-context.svg     Level 1: macula-desktop, who uses it and what it talks to
  c4-components.svg  Level 3: inside the one executable (src-tauri/src, dist/)

Every element is read from the code: src-tauri/src/*.rs, dist/, Cargo.toml,
install.sh and the release workflow. Colours are plain classes with a
prefers-color-scheme: dark block, no CSS custom properties (rsvg-convert
renders those black), the same style and legend as mcl-fovea's diagrams.
After a change, render each (rsvg-convert -w 1400 assets/c4-context.svg -o
/tmp/c.png) and look at it; keep architecture/README.md's tables in step.
"""
import os
from xml.sax.saxutils import escape

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")

SANS = '"Atkinson Hyperlegible", "Segoe UI", system-ui, -apple-system, Helvetica, Arial, sans-serif'
MONO = '"JetBrains Mono", ui-monospace, Menlo, Consolas, monospace'

# Element kinds, each (light fill, light stroke, dark fill, dark stroke).
KINDS = {
    "b-sys": ("#F5E9CC", "#A56F0A", "#3A2E13", "#E3AA3C"),   # in scope
    "b-per": ("#E3EBF2", "#3B5B7A", "#1D2A36", "#8EB4D8"),   # person
    "b-mac": ("#E6E4EE", "#5E5A78", "#26243A", "#A9A4C8"),   # Macula infrastructure
    "b-ext": ("#ECEFED", "#8A9791", "#1C2421", "#7D8B85"),   # external system
    "b-cmp": ("#FFFFFF", "#8A9791", "#171F1C", "#7D8B85"),   # component or container
}

STYLE = f"""<style>
.t-name {{ font-family: {SANS}; font-size: 15px; font-weight: 700; }}
.t-desc {{ font-family: {SANS}; font-size: 12.5px; }}
.t-rel {{ font-family: {SANS}; font-size: 11.5px; paint-order: stroke; stroke-width: 4px; stroke-linejoin: round; }}
.t-code {{ font-family: {MONO}; font-size: 13px; font-weight: 600; }}
.t-type {{ font-family: {MONO}; font-size: 11.5px; }}
.t-bound {{ font-family: {MONO}; font-size: 12px; font-weight: 600; }}
.t-leg {{ font-family: {SANS}; font-size: 12px; }}
.edge {{ fill: none; stroke-width: 1.25; }}
.edge-later {{ fill: none; stroke-width: 1.25; stroke-dasharray: 5 4; }}
.bg {{ fill: #FFFFFF; }}
""" + "".join(f".{k} {{ fill: {v[0]}; stroke: {v[1]}; stroke-width: {2 if k == 'b-sys' else 1.25}; }}\n" for k, v in KINDS.items()) + """\
.b-later { fill: none; stroke: #8A9791; stroke-width: 1.25; stroke-dasharray: 5 4; }
.bound { fill: none; stroke: #B3BDB8; stroke-width: 1; }
.head { fill: #3B5B7A; }
.t-name, .t-code { fill: #16201D; }
.t-type, .t-bound, .t-leg { fill: #66736E; }
.t-desc { fill: #34413C; }
.t-rel { fill: #66736E; stroke: #FFFFFF; }
.edge, .edge-later { stroke: #8A9791; }
.arrowhead { fill: #8A9791; }
@media (prefers-color-scheme: dark) {
.bg { fill: #171F1C; }
""" + "".join(f".{k} {{ fill: {v[2]}; stroke: {v[3]}; }}\n" for k, v in KINDS.items()) + """\
.b-later { stroke: #7D8B85; }
.bound { stroke: #4A5752; }
.head { fill: #8EB4D8; }
.t-name, .t-code { fill: #E6ECE9; }
.t-type, .t-bound, .t-leg { fill: #93A09A; }
.t-desc { fill: #C3CDC8; }
.t-rel { fill: #93A09A; stroke: #171F1C; }
.edge, .edge-later { stroke: #7D8B85; }
.arrowhead { fill: #7D8B85; }
}
</style>"""

DEFS = ('<defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" '
        'orient="auto-start-reverse"><path class="arrowhead" d="M0 0L10 5L0 10z"/></marker></defs>')

LEGEND = [("b-sys", "In scope"), ("b-per", "Person"), ("b-mac", "Macula infrastructure"),
          ("b-ext", "External system"), ("b-cmp", "Container or component"), ("b-later", "Planned, not built")]
BOUNDARY = ("bound", "Boundary")


class Diagram:
    def __init__(self, name, title, w, h):
        self.name, self.title, self.w, self.h, self.parts = name, title, w, h, []

    def text(self, cls, x, y, s, anchor=None, rotate=None):
        a = f' text-anchor="{anchor}"' if anchor else ""
        r = f' transform="rotate({rotate} {x} {y})"' if rotate is not None else ""
        self.parts.append(f'<text class="{cls}" x="{x}" y="{y}"{a}{r}>{escape(s)}</text>')

    def box(self, cls, x, y, w, h, name, kind, desc, person=False, code=False):
        self.parts.append(f'<rect class="{cls}" x="{x}" y="{y}" width="{w}" height="{h}" rx="10"/>')
        tx = x + 16
        if person:
            self.parts.append(f'<circle class="head" cx="{x + 26}" cy="{y + 24}" r="9"/>')
            tx = x + 44
        self.text("t-code" if code else "t-name", tx, y + 28, name)
        self.text("t-type", tx, y + 46, kind)
        for i, line in enumerate(desc):
            self.text("t-desc", x + 16, y + 70 + 16 * i, line)

    def bound(self, x, y, w, h, label):
        self.parts.append(f'<rect class="bound" x="{x}" y="{y}" width="{w}" height="{h}" rx="12"/>')
        self.text("t-bound", x + 16, y + 22, label)

    def edge(self, d, later=False):
        self.parts.append(f'<path class="{"edge-later" if later else "edge"}" d="{d}" marker-end="url(#a)"/>')

    def legend(self, y, boundary=True):
        x = 24
        for cls, label in LEGEND + ([BOUNDARY] if boundary else []):
            self.parts.append(f'<rect class="{cls}" x="{x}" y="{y}" width="22" height="14" rx="3"/>')
            self.text("t-leg", x + 30, y + 12, label)
            x += 34 + 6.6 * len(label)

    def write(self):
        svg = [(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {self.w} {self.h}" role="img" '
                f'aria-label="{escape(self.title)}">'), f"<title>{escape(self.title)}</title>", STYLE, DEFS,
               f'<rect class="bg" x="0" y="0" width="{self.w}" height="{self.h}"/>'] + self.parts + ["</svg>", ""]
        with open(os.path.join(OUT, self.name), "w") as f:
            f.write("\n".join(svg))



def context():
    d = Diagram("c4-context.svg", "macula-desktop system context", 1000, 860)
    d.box("b-per", 365, 30, 270, 96, "Operator", "[Person]",
          ["Runs the app on Linux, macOS or", "Windows; keyboard first"], person=True)
    d.box("b-ext", 690, 30, 270, 96, "GitHub Releases", "[External system]",
          ["One archive per OS and", "SHA256SUMS; install.sh checks it"])
    d.box("b-ext", 40, 200, 270, 120, "LLM endpoint", "[External: OpenAI-compatible]",
          ["The chat agent's model. DeepSeek", "by default; MACULA_LLM_BASE_URL", "and MACULA_LLM_MODEL override"])
    d.box("b-sys", 365, 200, 270, 120, "macula-desktop", "[Tauri 2 app, one executable]",
          ["A mesh node with a window: Mesh,", "Chat, Teams, Services, Realms tabs;", "tray mode keeps its presence"])
    d.box("b-ext", 690, 200, 270, 120, "System browser", "[External system]",
          ["Opens an application's http(s)", "URL the operator declared; the", "webview itself never does"])
    d.box("b-mac", 40, 430, 270, 120, "io.macula realm", "[Macula infrastructure]",
          ["The commons realm. Its public key", "is compiled in (io_macula.rs) and", "every provider is checked against it"])
    d.box("b-mac", 365, 430, 270, 120, "Macula fleet stations", "[Macula infrastructure]",
          ["Six stations, each pinned by the", "node_id it must prove; route", "pub/sub, calls and the DHT"])
    d.box("b-ext", 690, 430, 270, 120, "Other mesh agents", "[External systems]",
          ["Nodes beating on agent.hello", "(macula-mcp sessions and others):", "the roster, lobby and rooms"])
    d.box("b-ext", 365, 640, 270, 120, "Mesh services", "[External systems, mcl-*]",
          ["Procedures in the operating realm:", "mcl-rag/answer_query and", "add_knowledge for chat memory"])
    d.edge("M500 126V200"); d.text("t-rel", 508, 168, "uses: window, tray")
    d.edge("M635 78H690"); d.text("t-rel", 662, 70, "fetches", anchor="middle")
    d.edge("M365 260H310"); d.text("t-rel", 337, 250, "HTTPS,", anchor="middle"); d.text("t-rel", 337, 282, "SSE", anchor="middle")
    d.edge("M635 260H690"); d.text("t-rel", 662, 250, "opens", anchor="middle"); d.text("t-rel", 662, 282, "a URL", anchor="middle")
    d.edge("M560 320V430"); d.text("t-rel", 568, 368, "QUIC pool, wire 1.0,"); d.text("t-rel", 568, 384, "pq_hybrid node.key")
    d.edge("M420 320V375H175V430"); d.text("t-rel", 250, 367, "pins its key")
    d.edge("M635 490H690"); d.text("t-rel", 662, 480, "pub/sub", anchor="middle"); d.text("t-rel", 662, 512, "relay", anchor="middle")
    d.edge("M500 550V640"); d.text("t-rel", 508, 600, "calls by direct dial, operating realm")
    d.legend(815, boundary=False)
    d.write()


def components():
    d = Diagram("c4-components.svg", "macula-desktop components", 1240, 760)
    d.bound(270, 40, 700, 620, "macula-desktop executable [Tauri 2: src-tauri/, dist/]")
    d.box("b-cmp", 286, 70, 309, 120, "dist/", "[webview: index.html, app.js]",
          ["The five tabs and the chat. Every", "fact on screen arrives through", "invoke; it never touches the network."], code=True)
    d.box("b-cmp", 645, 70, 309, 120, "lib.rs run()", "[Tauri Builder, invoke_handler]",
          ["Registers 21 commands; manages", "MeshLink and ChatState; installs", "the tray at setup."], code=True)
    d.box("b-cmp", 286, 220, 309, 150, "chat.rs", "[personal agent loop]",
          ["Streams the model's answer as", "events. Its tool loop holds", "mesh_call, mesh_publish and",
           "content_put for the operator's yes;", "recall and remember via mcl-rag."], code=True)
    d.box("b-sys", 645, 220, 309, 150, "mesh.rs MeshLink", "[thread, own tokio runtime]",
          ["One Pool for the app's lifetime:", "fleet seeds, node.key, redial.", "agent.hello every 60 s, roster,",
           "lobby and joined rooms; publish,", "call, content; events to the window."], code=True)
    d.box("b-cmp", 286, 400, 151, 110, "apps.rs", "[apps.json]",
          ["Local and mesh", "apps; http(s) only", "to the browser"], code=True)
    d.box("b-cmp", 444, 400, 151, 110, "tray.rs", "[tray icon]",
          ["Close hides the", "window; Show or", "Quit from the tray"], code=True)
    d.box("b-cmp", 645, 400, 151, 110, "teams.rs", "[pure derivation]",
          ["The Teams board", "from joined rooms", "and lobby help"], code=True)
    d.box("b-cmp", 803, 400, 151, 110, "io_macula.rs", "[constants]",
          ["Six seeds pinned", "by node_id; the", "realm public key"], code=True)
    d.box("b-cmp", 286, 540, 309, 100, "Config directory", "[~/.config/macula-desktop]",
          ["node.key, settings.json, chat.jsonl,", "apps.json (%LOCALAPPDATA% on Windows)"])
    d.box("b-mac", 645, 540, 309, 100, "macula-rust 0.11", "[Macula SDK crate, crates.io]",
          ["pool, node_key, station_link, cbor,", "petname: the wire 1.0 node"])
    d.box("b-ext", 20, 220, 200, 120, "LLM endpoint", "[External]",
          ["OpenAI-compatible;", "DeepSeek by default"])
    d.box("b-ext", 20, 400, 200, 110, "System browser", "[External]",
          ["Opens a declared", "app's URL"])
    d.box("b-mac", 1020, 540, 200, 100, "Fleet stations", "[Macula infrastructure]",
          ["Six, pinned by", "node_id"])
    d.edge("M595 130H645"); d.text("t-rel", 620, 122, "invoke", anchor="middle")
    d.edge("M880 190V220"); d.text("t-rel", 888, 210, "commands")
    d.edge("M690 190V205H440V220"); d.text("t-rel", 565, 200, "commands", anchor="middle")
    d.edge("M595 300H645"); d.text("t-rel", 620, 292, "tools", anchor="middle")
    d.edge("M720 370V400"); d.text("t-rel", 728, 390, "stores")
    d.edge("M878 370V400"); d.text("t-rel", 886, 390, "seeds, key")
    d.edge("M799 370V540")
    d.edge("M440 370V540")
    d.text("t-rel", 448, 528, "settings, transcript")
    d.text("t-rel", 807, 528, "Pool")
    d.edge("M286 280H220"); d.text("t-rel", 245, 270, "HTTPS,", anchor="middle"); d.text("t-rel", 245, 298, "SSE", anchor="middle")
    d.edge("M286 455H220"); d.text("t-rel", 245, 445, "http(s)", anchor="middle"); d.text("t-rel", 245, 473, "URL", anchor="middle")
    d.edge("M954 590H1020"); d.text("t-rel", 987, 580, "QUIC", anchor="middle"); d.text("t-rel", 987, 608, "wire 1.0", anchor="middle")
    d.legend(700)
    d.write()


if __name__ == "__main__":
    for make in (context, components):
        make()
