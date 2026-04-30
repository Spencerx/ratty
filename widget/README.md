# `ratatui-ratty` 🐀

A [`ratatui`](https://github.com/ratatui/ratatui) widget for placing
inline 3D objects in [Ratty](https://github.com/orhun/ratty) through the
[Ratty Graphics Protocol](https://github.com/orhun/ratty/blob/main/protocols/graphics.md).

## Example

```rust,no_run
use std::io;

use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
use ratatui_ratty::{RattyGraphic, RattyGraphicSettings};

fn main() -> io::Result<()> {

    let graphic = RattyGraphic::new(
        RattyGraphicSettings::new("assets/objects/SpinyMouse.glb")
            .id(7)
            .animate(true)
            .scale(1.0),
    );
    graphic.register()?;

    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
    (&graphic).render(Rect::new(10, 5, 24, 10), &mut buf);

    Ok(())
}
```

The widget emits RGP APC sequences into the target buffer cell. Ratty then
resolves the asset and renders it as an inline 3D object anchored to that
terminal region.

## Examples

- [`examples/big_rat.rs`](https://github.com/orhun/ratty/tree/main/widget/examples/big_rat.rs): minimal inline object demo
- [`examples/document.rs`](https://github.com/orhun/ratty/tree/main/widget/examples/document.rs): TempleOS-inspired editor with embedded objects
- [`examples/draw.rs`](https://github.com/orhun/ratty/tree/main/widget/examples/draw.rs): 2D drawing pane with live 3D preview

## License

Licensed under [The MIT License](../LICENSE).
