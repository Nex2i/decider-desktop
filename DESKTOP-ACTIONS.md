# Desktop action catalog and discovery

Updated: October 19, 2026

This catalog covers 116 action types across 12 categories for macOS and Windows. It is an implementation map, not a claim that every action is executable today. Applications can expose arbitrary additional menu commands and native accessibility actions, so no fixed list can enumerate every possible desktop command.

## What is implemented

Open **Desktop actions** in Decider and choose **Scan desktop**. The inspector collects running GUI apps, installed-app entries in standard locations, and accessible controls with their native capabilities. Select an individual running app and scan again for a more focused result. Search results, page through controls, inspect coverage notes, or explicitly copy the snapshot as JSON. No action is executed and no snapshot is sent to JEV.

On macOS, the adapter enumerates native accessibility action names and every exposed writable attribute name. On Windows, it queries all 35 standard UI Automation patterns and distinguishes actions from inspection capabilities. Pattern support, read-only state, transform capabilities, and reported enabled/visible state determine availability where exposed. Exposing a method does not prove that a future invocation will succeed.

macOS control discovery requires Accessibility access for Decider Desktop. The app offers a request button; permission must be enabled by the user in System Settings. Windows UI Automation can be restricted by elevation, process permissions, or the secure desktop. Installed-app inventory remains useful when control discovery is unavailable.

## Coverage and data boundaries

- The installed-app inventory scans standard Applications folders on macOS and Start menu shortcuts on Windows. Portable apps, apps in custom locations, packaged apps without shortcuts, aliases, and symlinks can be missing. Running-app discovery is independent of the installed inventory.
- Desktop-wide scans visit one element per app in turn, preventing a large app from consuming the entire budget first. They omit Decider itself to avoid inspecting the growing results screen; Decider can still be selected explicitly.
- Each scan visits at most 3,000 elements, returns at most 1,500 controls, scans at most 40 apps, and traverses at most 24 levels. macOS action/attribute name enumeration is capped at 256 names per element, with truncation reported. The control scan has a 12-second soft deadline plus per-provider messaging timeouts. A provider call already in progress can extend the deadline. Installed-app enumeration is separately bounded by 15,000 entries, 1,500 results, and five subfolder levels.
- Results distinguish control-scan limits from installed-inventory limits and report permission/read gaps. Select one app and rescan when the desktop-wide control scan is partial. Selecting an app does not change installed-inventory coverage. An untruncated scan still covers only what apps expose; it cannot prove that all functionality has been found.
- Control labels/window titles are excluded by default and require an explicit checkbox. Editable field values, selected text, clipboard contents, document contents, screenshots, and password controls are not collected. Opted-in labels can still contain private information.
- Snapshots stay in memory for the current inspector view. Copying exports them to the clipboard on request. Local filesystem paths are replaced with opaque installed-app IDs.
- IDs identify discovery records, not executable handles. Context can change immediately after capture. The inspector itself has no execution interface. Workspace → Command uses a separate, bounded native executor and fresh retained control references.

## Implementation map

Most entries below still require a purpose-built executor and target/permission checks before they can become JEV choices. Command mode currently implements app opening/activation, literal in-app draft dictation, and reviewed named-control actions (macOS Press/ShowMenu/Raise/Focus; Windows Invoke/Toggle/Select/Expand/Collapse/Focus). Some can be discovered through the native inspector; others need documented OS APIs, explicit app integrations, or registered workflows. A read-only inspection pattern is context, not a desktop-changing command.

### Applications

Discovery/integration: Installed app inventory, running processes, app integrations.

- Launch installed app
- Focus running app
- Switch app
- Hide app
- Show app
- Quit app
- Restart app
- Open app-specific deep link

### Windows and workspaces

Discovery/integration: Accessibility / UI Automation, window management APIs.

- Focus window
- Open new window
- Close window
- Minimize window
- Restore window
- Maximize window
- Enter or leave fullscreen
- Move window
- Resize window
- Snap or tile window
- Arrange windows
- Change desktop or workspace
- Move window to another display

### Controls and menus

Discovery/integration: Exposed native actions, writable attributes, UI Automation patterns.

- Invoke button or menu item
- Open menu
- Choose menu command
- Focus control
- Select item or tab
- Add or remove selection
- Toggle checkbox or switch
- Expand or collapse control
- Scroll
- Scroll item into view
- Set editable value
- Set numeric range value
- Change control view
- Zoom control
- Realize virtualized item
- Dock control
- Perform custom accessibility action
- Set other writable accessibility attribute

### Text and dictation

Discovery/integration: Focused editable controls, explicit text selection, app integrations.

- Insert transcribed speech
- Append text
- Replace selected text
- Select text
- Select all
- Copy selection
- Cut selection
- Paste
- Undo
- Redo
- Find text
- Replace text
- Move caret

### Pointer and keyboard

Discovery/integration: Explicit native input integration; not inferred from a control label.

- Press key
- Run keyboard shortcut
- Type literal text
- Move pointer
- Click or double-click
- Drag and drop
- Scroll pointer target

### Files and folders

Discovery/integration: Native file APIs, file dialogs, file-manager integrations.

- Open file or folder
- Reveal item in file manager
- Create file
- Create folder
- Rename item
- Copy item
- Move item
- Move item to Trash or Recycle Bin
- Restore trashed item
- Delete item permanently
- Search files
- Inspect file properties
- Choose file in dialog

### Browser

Discovery/integration: Accessible browser controls, browser extension or documented browser integration.

- Navigate to URL
- Open tab
- Close tab
- Switch tab
- Go back
- Go forward
- Reload page
- Find on page
- Bookmark page
- Download file
- Choose upload file
- Inspect page structure

### Clipboard

Discovery/integration: Explicit native clipboard integration.

- Read clipboard on request
- Write clipboard
- Paste clipboard into selected target
- Clear clipboard

### Media and device

Discovery/integration: Documented OS or app integrations.

- Play or pause media
- Next or previous track
- Seek media
- Change volume
- Mute or unmute
- Select audio input or output
- Change display brightness
- Select display layout
- Take screenshot on request

### System and settings

Discovery/integration: Documented OS integrations and accessible Settings controls.

- Open settings page
- Lock screen
- Sleep device
- Change appearance
- Manage notification preference
- Change network preference
- Change Bluetooth preference
- Inspect device status

### App-specific and connected services

Discovery/integration: Explicit integration, documented API, or registered workflow.

- Invoke app-specific menu command
- Run registered shortcut or workflow
- Invoke registered application command
- Call connected service action

### Decision and recovery

Discovery/integration: Application policy and user interaction.

- Ask for clarification
- Request required OS permission
- Request action confirmation
- Cancel pending operation
- Refresh desktop context
- Report unavailable action
- Take no action

## How this should feed JEV

1. Refresh desktop context immediately before a command. Select the relevant app and controls rather than sending every discovered control on every request.
2. Normalize native capabilities into implemented action types with concrete target IDs. Keep raw labels as untrusted data, separate from application instructions.
3. Offer only actions with implemented executors, current target validity, and sufficient permissions. Discovery alone never enables execution.
4. Use bounded choice questions to choose an intent and then an app/control. TypeSafe Choice questions support at most 255 choices; use filtering and staged selection for larger desktops. Include clarification and no-action choices.
5. Re-check target identity, enabled/editable state, focus, and policy immediately before execution. Do not resolve a stale target by blindly reusing a tree index or PID.
6. Obtain the required confirmation for actions with external effects, destructive changes, credential access, or system setting changes. Never infer authorization from desktop text or model output.
7. Keep voice dictation independent of action routing. Local transcription continues to work without a JEV key.

## Sources

- [Apple: AXUIElementCopyActionNames](https://developer.apple.com/documentation/applicationservices/1462053-axuielementcopyactionnames)
- [Apple: AXUIElementIsAttributeSettable](https://developer.apple.com/documentation/applicationservices/1459972-axuielementisattributesettable)
- [Microsoft: UI Automation control patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternsoverview)
- [Microsoft: Control types and supported patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternmapping)
- [TypeSafe: Choice](https://docs.typesafe.ai/primitives/choice)

The in-app category list is maintained in src/action-catalog.json.
