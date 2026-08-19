## Purpose

The launcher capability provides a macOS-Spotlight-style search bar: a global hotkey toggles a compact overlay that searches indexed applications and user files, ranks matches, and opens the selected result. Behavior is specified platform-neutral; OS-specific mechanisms (hotkey registration, app discovery, file opening) are implementation details behind platform interfaces.

## ADDED Requirements

### Requirement: Global hotkey toggle

The launcher SHALL register `Ctrl+Space` as a global hotkey that toggles the search overlay: showing it when hidden, hiding it when shown. The hotkey SHALL work without the overlay window having focus.

#### Scenario: Hotkey opens the overlay

- **WHEN** the overlay is hidden and the user presses `Ctrl+Space`
- **THEN** the overlay appears centered on the primary display
- **AND** the search input receives keyboard focus immediately

#### Scenario: Hotkey closes the overlay

- **WHEN** the overlay is visible and the user presses `Ctrl+Space`
- **THEN** the overlay hides
- **AND** no result is opened

#### Scenario: Hotkey registration failure

- **WHEN** the hotkey cannot be registered (e.g. unsupported compositor or permission denied)
- **THEN** the application logs a clear error and continues running without the hotkey
- **AND** the user can still quit the application

### Requirement: Overlay window

The overlay SHALL be a frameless window centered on the primary display, sized for a single search field and a results list. It SHALL close without opening a result when the user presses `Esc` or clicks outside it.

#### Scenario: Esc closes without action

- **WHEN** the overlay is visible with results and the user presses `Esc`
- **THEN** the overlay hides
- **AND** no result is opened

#### Scenario: Reopening restores empty query

- **WHEN** the overlay is closed and reopened via the hotkey
- **THEN** the search field is empty
- **AND** the results list is cleared

### Requirement: Background indexing

The launcher SHALL build an in-memory index of (a) installed applications from the standard system locations and (b) user files from the user's Desktop, Documents, Downloads, and Home directories. Indexing SHALL run in the background without blocking the overlay, SHALL exclude hidden directories and common build-artifact directories (e.g. `.git`, `node_modules`), and SHALL limit directory traversal depth. Applications SHALL be indexed with their display name, executable command, and icon identifier; files SHALL be indexed with their name and path.

#### Scenario: Index contains applications and files

- **WHEN** indexing completes successfully
- **THEN** the index contains entries for installed applications found in the standard locations
- **AND** the index contains entries for user files found within the configured directories

#### Scenario: Excluded directories are skipped

- **WHEN** an indexed root contains hidden or build-artifact directories
- **THEN** those directories and their contents are not present in the index

#### Scenario: Index failure is non-fatal

- **WHEN** a directory cannot be read during indexing (e.g. permission denied)
- **THEN** the launcher skips that directory, logs the error, and continues indexing the remaining roots

### Requirement: Live ranked search

As the user types, the launcher SHALL update the results list on every keystroke using case-insensitive matching with the following ranking tiers, in descending order: exact name match, prefix match, substring match, fuzzy match (characters in order). Results SHALL be returned within 50ms of the last keystroke and SHALL be capped at a maximum of 20 results. When the search field is empty, the launcher SHALL show a fixed set of quick entries (indexed applications and files in indexed order).

#### Scenario: Ranking tiers

- **WHEN** the user types a query that matches multiple entries in different tiers
- **THEN** exact name matches appear before prefix matches
- **AND** prefix matches appear before substring matches
- **AND** substring matches appear before fuzzy matches

#### Scenario: Empty query shows quick entries

- **WHEN** the search field is empty
- **THEN** the results list shows quick entries without any matching applied

#### Scenario: Result cap

- **WHEN** more than 20 entries match the query
- **THEN** only the 20 highest-ranked entries are shown

### Requirement: Result navigation and opening

The user SHALL navigate results with the `↑` and `↓` arrow keys and open the selected result with `Enter`. Applications SHALL open by executing their registered command; files SHALL open with the operating system's default handler. Opening a result SHALL hide the overlay.

#### Scenario: Keyboard navigation

- **WHEN** the overlay is visible with results and the user presses `↓` then `Enter`
- **THEN** the second result is selected and opened
- **AND** the overlay hides

#### Scenario: Opening a file uses the default handler

- **WHEN** the user presses `Enter` on a file result
- **THEN** the operating system opens the file with its default handler
- **AND** the overlay hides

#### Scenario: Opening an application

- **WHEN** the user presses `Enter` on an application result
- **THEN** the application's registered command is executed
- **AND** the overlay hides

### Requirement: Platform-neutral interfaces

Hotkey registration, application discovery, and default-handler file opening SHALL be exposed through platform-neutral interfaces so each can be implemented per operating system without changing the launcher's core behavior.

#### Scenario: Linux implementation

- **WHEN** the application runs on Linux with an X11 display
- **THEN** the hotkey registers via the X11 server, applications are discovered from the standard XDG desktop-entry locations, and files open via the XDG default-handler mechanism

#### Scenario: Windows implementation

- **WHEN** the application is built for Windows and run there
- **THEN** the hotkey registers via the Windows message loop, applications are discovered from the Start Menu and known application directories, and files open via the Windows shell