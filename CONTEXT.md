# Glossary

## WindowWarden

The Windows application that applies per-application features based on which window is in the foreground: a cursor lock and a background mute.

## Managed application

An application configured by selecting a running process or entering an executable name or path. Its executable path identifies it when available; its process name is the fallback identity. These are two forms of the same setting, not separate kinds of managed application. An optional display name is shown instead of the identity; it does not affect matching.

## Cursor lock

The active restriction that keeps the cursor inside a lock target. It is removed when focus leaves the managed application.

## Lock target

The boundary used by a cursor lock: either the foreground application window or the monitor containing it.

## Config folder

The folder that holds `settings.json`. It is `%APPDATA%\WindowWarden` unless a custom folder is chosen; that choice is recorded in `location.json` in the default folder.

## Background mute

Muting a managed application's audio sessions while another window is in the foreground, and restoring them when it comes back. WindowWarden only restores the mutes it added; a mute the user set stays as it is.
