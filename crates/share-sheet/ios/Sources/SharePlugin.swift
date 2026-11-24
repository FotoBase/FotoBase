import SwiftRs
import Tauri
import UIKit

class ShareFilesArgs: Decodable {
  let files: [String]
}

class SharePlugin: Plugin {
  /// Opens one activity sheet with every file attached (TASK-09-005).
  @objc public func shareFiles(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ShareFilesArgs.self)
    if args.files.isEmpty {
      invoke.reject("no files to share")
      return
    }
    let urls = args.files.map { URL(fileURLWithPath: $0) }
    if let missing = urls.first(where: { !FileManager.default.fileExists(atPath: $0.path) }) {
      invoke.reject("file not found: \(missing.path)")
      return
    }
    DispatchQueue.main.async {
      guard let root = self.manager.viewController else {
        invoke.reject("no view controller to present the share sheet")
        return
      }
      let sheet = UIActivityViewController(activityItems: urls, applicationActivities: nil)
      // iPad presents the sheet as a popover and crashes without an anchor.
      UIUtils.centerPopover(rootViewController: root, popoverController: sheet)
      root.present(sheet, animated: true) {
        invoke.resolve()
      }
    }
  }
}

@_cdecl("init_plugin_share_sheet")
public func initPlugin() -> Plugin {
  return SharePlugin()
}
