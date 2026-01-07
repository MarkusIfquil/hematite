<div align="center">
  <h1>hematite: a simple, fast, opinionated X tiling window manager.</h1>
  <img src="https://github.com/MarkusIfquil/hematite/blob/main/images/hematite2.png?raw=true"/>
  <p></p>
  <img src="https://github.com/MarkusIfquil/hematite/blob/main/images/hematite1.png?raw=true"/>
</div>


# why hematite?
Hematite is designed to be as simple as possible while still having the functions of a modern tiling manager.

Hematite is based on [dwm](https://dwm.suckless.org/), and follows many of its philosophies, while also building on it with modern tooling and documentation. 

It is **simple**, only containing ~1500 lines of code, and is only concerned about tiling windows and showing a bar.

It is **fast** and **efficient**, refreshing only when necessary and contains as few moving parts as possible.

It is **opinionated**, contains no support for external scripting, with a minimal config file for appearance. It also contains only one tiling layout, which is master-stack.
# installation
## Build from source (recommended installation)
### clone the repository
```sh
git clone git@github.com:MarkusIfquil/hematite.git
```

### build from source
```sh
sudo cargo install --path . --root /usr
```

### add to .xinitrc (or any script that runs on startup)
```sh
exec hematite &
```

See the configuration section when running hematite for the first time as you will likely need to install/provide/replace certain programs that hematite assumes by default.

# dependencies

hematite is made to contain as few implementation dependencies (that you need to install yourself) as possible, but there are a few mandatory ones:

- xorg (runtime, build): libx11, xrandr, xorg-server, libxinerama
- sh (runtime): any posix-compliant shell for starting up and down commands
- rust (build): >= 1.74.0

# next steps
## status bar
included in the repository is a bar script. adding it to your startup script displays status information on the bar.

### add to .xinitrc
```sh
bash bar.sh &
```
## notifications
`dunst` is recommended for showing notifications as it is also simple and lightweight.
## install dunst
### Arch linux:
```sh
sudo pacman -S dunst
```
### add to .xinitrc
```sh
dunst &
```
## background image
`feh` is recommended for setting the wallpaper.
### install feh
### Arch linux:
```sh
sudo pacman -S feh
```
### add to .xinitrc
```sh
~/.fehbg &
```
# configuration
configuration is set using the `config.toml` file located in your `.config/hematite` folder. A default one is provided when hematite is run for the first time.
## font
FreeSans is used by default due to compatibility, but it is recommended to change it to a different font (for example [Jetbrains Mono Nerd](https://www.nerdfonts.com/). For TTF fonts the install path is usually `/usr/share/fonts/TTF/{font_name}.ttf`.
## hotkeys
not all keys are supported by default. If you want to use a non-character key then you will have to add it manually in the code.

# default hotkeys
| Keybinding           | Description                                                            |
| -------------------- | ---------------------------------------------------------------------- |
| Mod + (1-9)          | Switch to a desktop/tag                                                |
| Shift + Mod + (1-9)  | Move window to a desktop/tag                                           |
| Mod + q              | Close window                                                           |
| Shift + Mod + q      | Exit hematite                                                          |
| Mod + h              | Decrease master area ratio                                             |
| Mod + j              | Increase stack area ratio                                              |
| Mod + k              | Focus previous window                                                  |
| Mod + l              | Focus next window                                                      |
| Mod + Left           | Switch to previous desktop/tag                                         |
| Mod + Right          | Switch to next desktop/tag                                             |
| Mod + Enter          | Swap focused window with master window                                 |
| Mod + c              | Application launcher (default: rofi drun)                              |
| Control + Mod + Enter| Open terminal (default: alacritty)                                     |
| Control + Mod + l    | Open browser (default: librewolf)                                      |
| Mod + u              | Take screenshot (default: maim)                                        |
