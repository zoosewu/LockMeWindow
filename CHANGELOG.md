# Changelog

## [0.4.0](https://github.com/zoosewu/LockMeWindow/compare/v0.3.1...v0.4.0) (2026-09-16)


### ⚠ BREAKING CHANGES

* the executable is now window-warden.exe and settings live in %APPDATA%\WindowWarden. On the first run under the new name the settings are copied from the old folder, the old startup entry is removed, and startup is registered again for the new executable when it was enabled.

### Added

* redraw the icon set and follow the taskbar theme ([5e40879](https://github.com/zoosewu/LockMeWindow/commit/5e40879fc3f76e945db239ec4d3a8f69f273c1b5))
* rename the project to WindowWarden ([170b95f](https://github.com/zoosewu/LockMeWindow/commit/170b95f409f48f9654e076fb6ea30dacee0ace78))

## [0.3.1](https://github.com/zoosewu/LockMeWindow/compare/v0.3.0...v0.3.1) (2026-09-16)


### Internal

* release 0.3.1 ([38ee9af](https://github.com/zoosewu/LockMeWindow/commit/38ee9af19aadf1a19e9f9d60c1d6149fa2b68a17))

## [0.3.0](https://github.com/zoosewu/LockMeWindow/compare/v0.2.0...v0.3.0) (2026-09-16)


### Added

* add display names, language setting and clearer wording ([33b63b6](https://github.com/zoosewu/LockMeWindow/commit/33b63b6634aff3601808ece7cfa426ce8ac7e74b))

## [0.2.0](https://github.com/zoosewu/LockMeWindow/compare/v0.1.1...v0.2.0) (2026-09-15)


### Added

* redesign UI with Slint, import/export and config folder ([3be0386](https://github.com/zoosewu/LockMeWindow/commit/3be0386bbaefa38adec6314d1fd612623f54e961))

## [0.1.1](https://github.com/zoosewu/LockMeWindow/compare/v0.1.0...v0.1.1) (2026-09-15)


### Fixed

* start with Windows without administrator rights ([237ce56](https://github.com/zoosewu/LockMeWindow/commit/237ce566de10e0f37cd5e076cb3c7c9087658420))

## 0.1.0 (2026-09-15)


### Added

* add Windows startup checkbox ([4ca0ef6](https://github.com/zoosewu/LockMeWindow/commit/4ca0ef649533e6f1c25c5ea959e5b12b74de27d4))
* enforce single instance and add app icon ([88cee3d](https://github.com/zoosewu/LockMeWindow/commit/88cee3d85b28ea337a1473b71d8cca5d23929ac9))
* implement LockMeWindow ([fdb00c0](https://github.com/zoosewu/LockMeWindow/commit/fdb00c06d331e9a724abb0fa439ff1a7701fcfa8))


### Fixed

* allow taskbar recreation message ([2990b7f](https://github.com/zoosewu/LockMeWindow/commit/2990b7f6a0563f4d75b80da1ffffc7a9f4a089da))
* close main window to tray ([e94baa2](https://github.com/zoosewu/LockMeWindow/commit/e94baa283eec231d02e36ba842f12f30f744da51))
* keep tray icon visible ([96723a6](https://github.com/zoosewu/LockMeWindow/commit/96723a6a3b24d488ed8324310fcd988610658403))
* reconcile cursor lock state ([3fcf4f7](https://github.com/zoosewu/LockMeWindow/commit/3fcf4f755fa30ea4c7aba04c154b40132dc9851a))
* survive startup before Explorer ([2d50612](https://github.com/zoosewu/LockMeWindow/commit/2d506125c30fe87157d0ddaa1b1dbc3cef3bfc56))


### Internal

* initialize repository ([1b67314](https://github.com/zoosewu/LockMeWindow/commit/1b673149d75010c3d6cada80be22d6889d531331))
