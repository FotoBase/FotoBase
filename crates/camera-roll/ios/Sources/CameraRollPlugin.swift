import Photos
import SwiftRs
import Tauri
import UIKit

class ObserveArgs: Decodable {
  let onChange: Channel
}

struct AccessReply: Encodable {
  let state: String
}

struct FoldersReply: Encodable {
  let folders: [String]
}

/// Keeps an app-owned mirror folder in step with the photo library and tells
/// Rust when it changed (TASK-09-006). A `PHAsset` has no file path, so the
/// scanner reads the mirror. The library itself is only read, never written.
/// Event-driven: the only trigger is `PHPhotoLibraryChangeObserver`.
class CameraRollPlugin: Plugin, PHPhotoLibraryChangeObserver {
  private var channel: Channel?
  private var fetch: PHFetchResult<PHAsset>?
  private let queue = DispatchQueue(label: "fotobase.camera-roll", qos: .utility)
  private let mirror: URL = {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    return base.appendingPathComponent("camera-roll", isDirectory: true)
  }()

  @objc public func requestAccess(_ invoke: Invoke) {
    PHPhotoLibrary.requestAuthorization(for: .readWrite) { status in
      invoke.resolve(AccessReply(state: CameraRollPlugin.name(of: status)))
    }
  }

  private static func name(of status: PHAuthorizationStatus) -> String {
    switch status {
    case .authorized: return "granted"
    case .limited: return "limited"
    default: return "denied"
    }
  }

  @objc public func startObserving(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ObserveArgs.self)
    let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
    guard status == .authorized || status == .limited else {
      invoke.reject("photo library access not granted")
      return
    }
    stopObserving()
    channel = args.onChange
    do {
      try FileManager.default.createDirectory(at: mirror, withIntermediateDirectories: true)
      var dir = mirror
      var values = URLResourceValues()
      values.isExcludedFromBackup = true
      try? dir.setResourceValues(values)
    } catch {
      invoke.reject("cannot create the camera roll mirror: \(error.localizedDescription)")
      return
    }
    let options = PHFetchOptions()
    options.predicate = NSPredicate(
      format: "mediaType == %d OR mediaType == %d",
      PHAssetMediaType.image.rawValue, PHAssetMediaType.video.rawValue)
    let result = PHAsset.fetchAssets(with: options)
    fetch = result
    PHPhotoLibrary.shared().register(self)
    // Answer at once; the first copy runs in the background and announces
    // itself on the channel, so a large library never blocks startup.
    invoke.resolve(FoldersReply(folders: [mirror.path]))
    queue.async {
      var all: [PHAsset] = []
      result.enumerateObjects { asset, _, _ in all.append(asset) }
      self.sync(upserting: all, keeping: Set(all.map { $0.localIdentifier }))
    }
  }

  @objc public func stopObserving(_ invoke: Invoke) {
    stopObserving()
    invoke.resolve()
  }

  private func stopObserving() {
    PHPhotoLibrary.shared().unregisterChangeObserver(self)
    channel = nil
    fetch = nil
  }

  func photoLibraryDidChange(_ change: PHChange) {
    guard let old = fetch, let details = change.changeDetails(for: old) else { return }
    fetch = details.fetchResultAfterChanges
    let touched = details.insertedObjects + details.changedObjects
    var live = Set<String>()
    details.fetchResultAfterChanges.enumerateObjects { asset, _, _ in
      live.insert(asset.localIdentifier)
    }
    queue.async { self.sync(upserting: touched, keeping: live) }
  }

  /// Copies the original of each asset in `assets` (an edited asset is
  /// copied again) and deletes mirror files of assets no longer in `live`.
  private func sync(upserting assets: [PHAsset], keeping live: Set<String>) {
    let fm = FileManager.default
    var changed = false
    for asset in assets {
      guard let resource = CameraRollPlugin.original(of: asset) else { continue }
      let target = mirror.appendingPathComponent(
        "\(CameraRollPlugin.safe(asset.localIdentifier))_\(resource.originalFilename)")
      let stamp = asset.modificationDate ?? asset.creationDate
      if let attrs = try? fm.attributesOfItem(atPath: target.path),
        let mtime = attrs[.modificationDate] as? Date, let stamp = stamp, mtime >= stamp
      {
        continue
      }
      // An edit swaps the original for the edited resource: drop the old name.
      let prefix = CameraRollPlugin.safe(asset.localIdentifier) + "_"
      if let names = try? fm.contentsOfDirectory(atPath: mirror.path) {
        for name in names where name.hasPrefix(prefix) && name != target.lastPathComponent {
          try? fm.removeItem(at: mirror.appendingPathComponent(name))
          changed = true
        }
      }
      let options = PHAssetResourceRequestOptions()
      // Never pull from iCloud in the background: it costs data and battery.
      options.isNetworkAccessAllowed = false
      let done = DispatchSemaphore(value: 0)
      var ok = false
      try? fm.removeItem(at: target)
      PHAssetResourceManager.default().writeData(for: resource, toFile: target, options: options) {
        error in
        ok = (error == nil)
        done.signal()
      }
      done.wait()
      changed = changed || ok
    }
    if let files = try? fm.contentsOfDirectory(atPath: mirror.path) {
      let keepPrefixes = live.map { CameraRollPlugin.safe($0) + "_" }
      for name in files where !keepPrefixes.contains(where: { name.hasPrefix($0) }) {
        try? fm.removeItem(at: mirror.appendingPathComponent(name))
        changed = true
      }
    }
    if changed {
      try? channel?.send(FoldersReply(folders: [mirror.path]))
    }
  }

  /// The edited version when there is one, else the original.
  private static func original(of asset: PHAsset) -> PHAssetResource? {
    let resources = PHAssetResource.assetResources(for: asset)
    let wanted: [PHAssetResourceType] =
      asset.mediaType == .video ? [.fullSizeVideo, .video] : [.fullSizePhoto, .photo]
    for type in wanted {
      if let r = resources.first(where: { $0.type == type }) { return r }
    }
    return nil
  }

  private static func safe(_ id: String) -> String {
    return id.replacingOccurrences(of: "/", with: "-")
  }
}

@_cdecl("init_plugin_camera_roll")
public func initPlugin() -> Plugin {
  return CameraRollPlugin()
}
