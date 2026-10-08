# App icon

Two smooth, asymmetric pebbles overlap on a warm ivory background. The sage
green and clay colors extend the existing in-app mark. The pairing reflects
pebbling: a small object offered as a gesture of attention.

## Editable source

`macos/Pebble/Resources/Pebble.icon` is the source of truth. It is an Icon
Composer document containing `icon.json` and two 1024 × 1024 SVG layers.
The artwork has flat fills, transparent surroundings, and no baked lighting
or outer mask. Icon Composer supplies those effects.

| Setting | Sage pebble | Clay pebble |
| --- | --- | --- |
| Artwork color | `#597563` | `#C4997A` |
| Group | Separate, Individual mode | Separate, Individual mode |
| Glass effects / specular | Enabled | Enabled |
| Blur material | 50% | 50% |
| Translucency | 20% | 25% |
| Neutral shadow | 35% | 35% |
| Mono fill | 84% white | White |

The background uses an automatic gradient from an ivory base
(`sRGB 0.935, 0.915, 0.875`). Default and Dark retain the stone colors. The Mono
overrides provide brighter source fills for the system's Clear and Tinted
appearances. Actual tinted colors depend on the user's system tint.

## Edit and preview

```sh
just icon          # open the layered document in Icon Composer
just preview-icon # export six appearances plus 32, 64, and 256 px previews
just build         # compile the icon and app
```

Adjust glass materials in Icon Composer and save the document before building.
Close the document before editing its SVG files externally: saving an open
document can overwrite external asset edits with its loaded copies.

`preview-icon` uses the `ictool` bundled inside the selected Xcode's Icon
Composer app. Generated previews stay in the ignored `build/icon-preview/`
directory. The exported images are review artifacts; the app uses the layered
document.

## Xcode integration

`macos/project.yml` includes the package as one `wrapper.icon` resource and
sets the **target-level** `ASSETCATALOG_COMPILER_APPICON_NAME` to `Pebble`.
This must be set on the target because XcodeGen's application preset supplies
its own default icon name.

Xcode compiles `Assets.car`, generates `Pebble.icns` for older systems, and
writes `CFBundleIconName` and `CFBundleIconFile`. Keep these compiled files out
of source control. The app still targets macOS 14; the generated fallback has
been inspected, but older macOS versions have not been exercised.

The workflow follows Apple's
[Create icons with Icon Composer](https://developer.apple.com/videos/play/wwdc2025/361/)
guidance: export flat layers, apply materials in Icon Composer, and deliver
the `.icon` document to Xcode.
