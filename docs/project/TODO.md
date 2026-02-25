scan to improve panic and exception handling like force unwrap and expect.

---

implement self update logic for vtcode.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

--

improve and fix syntax highlighting in raw markdown code
/Users/vinhnguyenxuan/Documents/vtcode-resources/wrong_syntax_highlight.png

---

check error
https://github.com/vinhnx/VTCode/issues/605#issuecomment-3942895952

---

---

idea: maybe align plan_task_tracker mode with task_tracker as subsystem for agent planning and execution flow.

---

improve system prompt "/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/plans/improve-system-prompt-v2.md"

---

## Summary

Goal: improve speed, token efficiency, and tool-call reliability while
minimizing regression risk.
Strategy: fix correctness bugs first, then reduce prompt/tool overhead with
config-compatible, surgical changes.

## Key Findings Driving This Plan

- is_valid_tool does not enforce policy result (it calls policy check and
  ignores return), causing false “valid” decisions and avoidable failures/
  retries.
- Core runner path builds full tool declarations without using cached/mode-
  aware docs (tool_documentation_mode), wasting tokens and startup work.
- Prompt/tool guidance has naming drift (ask_user_question vs ask_questions vs
  request_user_input) and legacy references, increasing tool-call mistakes.
- STRUCTURED_REASONING_INSTRUCTIONS is always appended, including lightweight/
  minimal flows, adding fixed token overhead.
- Loop/retry behavior is split across layers, with some non-retryable failures
  still retried.

--

maybe remove whole tree-sitter

---

reference plan prososal structure

```
⏺ Now I have everything I need. Let me write the plan.

⏺ Updated plan
  ⎿  /plan to preview

────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
 Ready to code?

 Here is VT Code's plan:
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 Plan: Convert CFRequestTurnOnLocationViewController to SwiftUI

 Context

 CFRequestTurnOnLocationViewController is a UIKit PanModal bottom sheet that prompts users to enable location services. It is presented from CFDashboardViewController via presentPanModal(). The conversion
 replaces the UIKit implementation with a SwiftUI View + ViewModel following the project's MVVM + Combine + CDS pattern, while keeping backward compatibility for the UIKit presenter.

 ---
 Files to Create

 1. CFRequestTurnOnLocationViewModel.swift

 Path: AppFeatures/CTFeed/CTFeed/Features/UpdateLocation/CFRequestTurnOnLocationViewModel.swift

 - State struct (CFRequestTurnOnLocationState):
   - isAllow: Bool = false — tracks if user tapped "Allow"
 - Input enum (CFRequestTurnOnLocationInput):
   - allowTapped — user tapped the "Allow" button
   - closeTapped — user tapped "Back" or the back icon button
 - ViewModel class (CFRequestTurnOnLocationViewModel: ViewModel):
   - @Published var state
   - var onDismiss: ((Bool) -> Void)? — callback to forward result to UIKit caller
   - @Published var shouldDismiss: Bool = false — triggers dismiss from the view
   - Uses PassthroughRelay for allowTappedStream and closeTappedStream
   - On .allowTapped: sets state.isAllow = true, calls CTLocationService.shared.requestWhenInUseAuthorization(), sets shouldDismiss = true
   - On .closeTapped: sets shouldDismiss = true

 ---
 2. CFRequestTurnOnLocationView.swift

 Path: AppFeatures/CTFeed/CTFeed/Features/UpdateLocation/CFRequestTurnOnLocationView.swift

 SwiftUI View conforming to the CDS + MVVM pattern:

 CFRequestTurnOnLocationView
 ├── VStack
 │   ├── Header bar (title centered + back button leading)
 │   ├── Header text  → Text(...).cdsTextStyle(.displayPage)
 │   ├── Content text → Text(...).cdsTextStyle(.bodySection) + CTColor.gray600
 │   ├── Image        → Image(uiImage: R.image.img_location_share() ?? .init())
 │   ├── Divider      → Divider() with CTColor.gray200
 │   ├── Allow button → Button(...).cdsButtonStyle(.primary, size: .medium)
 │   └── Back button  → Button(...).cdsButtonStyle(.secondary, size: .medium)

 - @ObservedObject var viewModel: AnyViewModel<CFRequestTurnOnLocationState, CFRequestTurnOnLocationInput>
 - @Environment(\.colorTheme) private var colorTheme
 - @Environment(\.dismiss) private var dismiss
 - Uses .onChange(of: viewModel.shouldDismiss) to trigger dismiss()
 - Background: Color.white
 - All localized strings via CFLocalize.*

 ---
 Files to Modify

 3. CFDashboardViewController.swift

 Path: AppFeatures/CTFeed/CTFeed/Features/Dashboard/CFDashboardViewController.swift (~line 369)

 Replace the presentPanModal(viewController) call with a UIHostingController wrapper:

 // Before
 let viewController = CFRequestTurnOnLocationViewController()
 viewController.completion = { [weak self] isAllow in
     self?.listener?.shouldRequestLocationAgain = isAllow
 }
 presentPanModal(viewController)

 // After
 let vm = CFRequestTurnOnLocationViewModel()
 vm.onDismiss = { [weak self] isAllow in
     self?.listener?.shouldRequestLocationAgain = isAllow
 }
 let view = CFRequestTurnOnLocationView(viewModel: vm.eraseToAnyViewModel())
 let hostingVC = UIHostingController(rootView: view)
 hostingVC.modalPresentationStyle = .overCurrentContext
 hostingVC.view.backgroundColor = .clear
 present(hostingVC, animated: true)

 ---
 Architecture Diagram

 graph TD
     A[CFDashboardViewController] -->|creates & presents| B[UIHostingController]
     B --> C[CFRequestTurnOnLocationView]
     C -->|@ObservedObject| D[AnyViewModel]
     D --> E[CFRequestTurnOnLocationViewModel]
     E -->|allowTapped| F[CTLocationService.shared]
     E -->|onDismiss callback| A

 ---
 Key Patterns Referenced

 ┌─────────────────────────────┬────────────────────────────────────────────────────┐
 │           Pattern           │                    Source File                     │
 ├─────────────────────────────┼────────────────────────────────────────────────────┤
 │ AnyViewModel + State/Input  │ AVPhotoViewerScreen.swift                          │
 ├─────────────────────────────┼────────────────────────────────────────────────────┤
 │ @Environment(\.colorTheme)  │ AVPhotoViewerScreen.swift, CICChatRoomScreen.swift │
 ├─────────────────────────────┼────────────────────────────────────────────────────┤
 │ .cdsButtonStyle(.primary)   │ references/components_api.md                       │
 ├─────────────────────────────┼────────────────────────────────────────────────────┤
 │ .cdsTextStyle(.bodySection) │ references/cds_components.md                       │
 ├─────────────────────────────┼────────────────────────────────────────────────────┤
 │ PassthroughRelay for inputs │ references/architecture.md                         │
 └─────────────────────────────┴────────────────────────────────────────────────────┘

 ---
 Verification

 1. Build the CTFeed target — no compile errors
 2. Tap the location banner in the Feed dashboard — the bottom sheet appears with correct layout
 3. Tap "Allow" — location authorization prompt appears, sheet dismisses without animation, shouldRequestLocationAgain = true is set on listener
 4. Tap "Back" or back icon — sheet dismisses with animation, shouldRequestLocationAgain = false
 5. Verify light/dark mode theming renders correctly
```

(THIS is for TUI message for agent to request user persmission to execute plan. The plan content is not important, just the structure and flow of the message.)

```
VT Code has written up a plan and is ready to execute. Would you like to proceed?

❯ 1. Yes, clear context (25% used) and auto-accept edits (shift+tab)
  2. Yes, auto-accept edits
  3. Yes, manually approve edits
  4. Type here to tell Claude what to change

ctrl-g to edit in VS Code · ~/.claude/plans/piped-beaming-puppy.md
```

---

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/docs/BLOATY_ANALYSIS.md

---

extract tui modules to separate files for better organization and maintainability. Each module can have its own file with clear naming conventions, making it easier to navigate and manage the codebase as the project grows. and also to reduce the size of the main tui file, improving readability and reducing cognitive load when working on specific features or components.

also consider open sourcing the tui modules as a standalone library for other projects to use, which would require further refactoring and documentation to ensure it is reusable and adaptable to different contexts.
