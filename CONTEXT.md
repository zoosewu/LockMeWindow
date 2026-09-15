# Glossary

## LockMeWindow

The Windows application that automatically confines the cursor while a managed application is in the foreground.

## Managed application

An application configured by selecting a running process or entering an executable name or path. Its executable path identifies it when available; its process name is the fallback identity. These are two forms of the same setting, not separate kinds of managed application.

## Cursor lock

The active restriction that keeps the cursor inside a lock target. It is removed when focus leaves the managed application.

## Lock target

The boundary used by a cursor lock: either the foreground application window or the monitor containing it.

## Config folder

The folder that holds `settings.json`. It is `%APPDATA%\LockMeWindow` unless a custom folder is chosen; that choice is recorded in `location.json` in the default folder.
