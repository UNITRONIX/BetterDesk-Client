import 'dart:io';

import 'package:flutter/painting.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:get/get.dart';
import 'package:path/path.dart' as path;
import 'package:path_provider/path_provider.dart';

const int kBrandingLogoMaxBytes = 512 * 1024;
const String kBrandingLogoFileName = 'betterdesk_branding_logo';

final _brandingLogoExts = <String>{'.png', '.jpg', '.jpeg', '.webp'};

class BrandingModel {
  static BrandingModel get current {
    if (!Get.isRegistered<BrandingModel>()) {
      Get.put(BrandingModel(), permanent: true);
    }
    return Get.find<BrandingModel>();
  }

  final companyName = ''.obs;
  final phone = ''.obs;
  final email = ''.obs;
  final website = ''.obs;
  final hasLogo = false.obs;
  final logoPath = ''.obs;
  final logoEpoch = 0.obs;
  final accentColor = ''.obs;
  final backgroundColor = ''.obs;

  BrandingModel() {
    load();
  }

  bool get hasCompanyName => companyName.value.trim().isNotEmpty;

  bool get hasContactOrLogo =>
      hasLogo.value ||
      phone.value.trim().isNotEmpty ||
      email.value.trim().isNotEmpty ||
      website.value.trim().isNotEmpty;

  bool get hasBranding => hasCompanyName || hasContactOrLogo;

  bool get isManagedByServer =>
      bind.mainGetLocalOption(key: kOptionBrandingSource) ==
      kBrandingSourceServer;

  String get managedRevision =>
      bind.mainGetLocalOption(key: kOptionBrandingRevision);

  void load() {
    companyName.value =
        bind.mainGetLocalOption(key: kOptionBrandingCompanyName);
    phone.value = bind.mainGetLocalOption(key: kOptionBrandingPhone);
    email.value = bind.mainGetLocalOption(key: kOptionBrandingEmail);
    website.value = bind.mainGetLocalOption(key: kOptionBrandingWebsite);
    accentColor.value =
        bind.mainGetLocalOption(key: kOptionBrandingAccentColor);
    backgroundColor.value =
        bind.mainGetLocalOption(key: kOptionBrandingBackgroundColor);
    final logoFlag = bind.mainGetLocalOption(key: kOptionBrandingLogo);
    if (logoFlag == 'Y') {
      _resolveLogoPath().then((p) {
        if (p != null && File(p).existsSync()) {
          logoPath.value = p;
          hasLogo.value = true;
          logoEpoch.value++;
        } else {
          logoPath.value = '';
          hasLogo.value = false;
        }
      });
    } else {
      logoPath.value = '';
      hasLogo.value = false;
    }
  }

  Future<String?> _resolveLogoPath() async {
    final managedPath = bind.mainGetLocalOption(key: kOptionBrandingLogoPath);
    if (managedPath.isNotEmpty && File(managedPath).existsSync()) {
      return managedPath;
    }
    final dir = await getApplicationSupportDirectory();
    if (!await dir.exists()) return null;
    File? newest;
    DateTime? newestTime;
    await for (final entity in dir.list()) {
      if (entity is! File) continue;
      if (!_isBrandingLogoFile(entity.path)) continue;
      final modified = await entity.lastModified();
      if (newestTime == null || modified.isAfter(newestTime)) {
        newest = entity;
        newestTime = modified;
      }
    }
    return newest?.path;
  }

  bool _isBrandingLogoFile(String filePath) {
    final name = path.basename(filePath);
    if (!name.startsWith(kBrandingLogoFileName)) return false;
    return _brandingLogoExts.contains(path.extension(name).toLowerCase());
  }

  void _evictLogoImage(String filePath) {
    if (filePath.isEmpty) return;
    try {
      PaintingBinding.instance.imageCache.evict(FileImage(File(filePath)));
    } catch (_) {}
  }

  Future<void> _deleteLogoFiles({String? keepPath}) async {
    final dir = await getApplicationSupportDirectory();
    if (!await dir.exists()) return;
    await for (final entity in dir.list()) {
      if (entity is! File) continue;
      if (!_isBrandingLogoFile(entity.path)) continue;
      if (keepPath != null &&
          path.equals(entity.path, keepPath)) {
        continue;
      }
      _evictLogoImage(entity.path);
      try {
        await entity.delete();
      } catch (_) {
        // Windows may briefly lock the previous Image.file; ignore and
        // leave orphan files for the next successful replace.
      }
    }
  }

  /// Returns an error translation key on failure, or null on success.
  Future<String?> setLogoFromPath(String sourcePath) async {
    final ext = path.extension(sourcePath).toLowerCase();
    if (!_brandingLogoExts.contains(ext)) {
      return 'Invalid logo format';
    }
    final src = File(sourcePath);
    if (!await src.exists()) {
      return 'Invalid logo format';
    }
    final len = await src.length();
    if (len <= 0 || len > kBrandingLogoMaxBytes) {
      return 'Logo too large';
    }

    final previousPath = logoPath.value;
    final dir = await getApplicationSupportDirectory();
    if (!await dir.exists()) {
      await dir.create(recursive: true);
    }

    // Unique destination avoids Flutter Image cache + Windows file locks
    // when replacing a logo that is still displayed from the old path.
    final stamp = DateTime.now().millisecondsSinceEpoch;
    final dest =
        path.join(dir.path, '${kBrandingLogoFileName}_$stamp$ext');
    await src.copy(dest);

    await bind.mainSetLocalOption(key: kOptionBrandingLogo, value: 'Y');
    logoPath.value = dest;
    hasLogo.value = true;
    logoEpoch.value++;

    if (previousPath.isNotEmpty) {
      _evictLogoImage(previousPath);
    }
    await _deleteLogoFiles(keepPath: dest);
    return null;
  }

  Future<void> removeLogo() async {
    final previousPath = logoPath.value;
    logoPath.value = '';
    hasLogo.value = false;
    logoEpoch.value++;
    if (previousPath.isNotEmpty) {
      _evictLogoImage(previousPath);
    }
    await _deleteLogoFiles();
    await bind.mainSetLocalOption(key: kOptionBrandingLogo, value: '');
  }

  Future<void> save({
    required String company,
    required String phoneValue,
    required String emailValue,
    required String websiteValue,
    required String accentValue,
    required String backgroundValue,
  }) async {
    final c = company.trim();
    final p = phoneValue.trim();
    final e = emailValue.trim();
    final w = websiteValue.trim();
    final accent = accentValue.trim();
    final background = backgroundValue.trim();
    await bind.mainSetLocalOption(key: kOptionBrandingCompanyName, value: c);
    await bind.mainSetLocalOption(key: kOptionBrandingPhone, value: p);
    await bind.mainSetLocalOption(key: kOptionBrandingEmail, value: e);
    await bind.mainSetLocalOption(key: kOptionBrandingWebsite, value: w);
    await bind.mainSetLocalOption(key: kOptionBrandingAccentColor, value: accent);
    await bind.mainSetLocalOption(
        key: kOptionBrandingBackgroundColor, value: background);
    companyName.value = c;
    phone.value = p;
    email.value = e;
    website.value = w;
    accentColor.value = accent;
    backgroundColor.value = background;
  }

  Future<void> clear() async {
    await bind.mainSetLocalOption(key: kOptionBrandingCompanyName, value: '');
    await bind.mainSetLocalOption(key: kOptionBrandingPhone, value: '');
    await bind.mainSetLocalOption(key: kOptionBrandingEmail, value: '');
    await bind.mainSetLocalOption(key: kOptionBrandingWebsite, value: '');
    await bind.mainSetLocalOption(key: kOptionBrandingAccentColor, value: '');
    await bind.mainSetLocalOption(
        key: kOptionBrandingBackgroundColor, value: '');
    await removeLogo();
    companyName.value = '';
    phone.value = '';
    email.value = '';
    website.value = '';
    accentColor.value = '';
    backgroundColor.value = '';
  }

  /// Branding accent, or [fallback] when the stored value is empty or not hex.
  Color accentOr(Color fallback) =>
      parseHexColor(accentColor.value) ?? fallback;

  /// Branding element background, or [fallback] when unset or invalid.
  Color backgroundOr(Color fallback) =>
      parseHexColor(backgroundColor.value) ?? fallback;

  /// `#RRGGBB` or `#AARRGGBB` (leading `#` optional).
  static Color? parseHexColor(String raw) {
    var s = raw.trim();
    if (s.isEmpty) return null;
    if (s.startsWith('#')) s = s.substring(1);
    if (s.length == 6) s = 'FF$s';
    if (s.length != 8) return null;
    final value = int.tryParse(s, radix: 16);
    if (value == null) return null;
    return Color(value);
  }

  static String normalizeWebsiteUrl(String raw) {
    final trimmed = raw.trim();
    if (trimmed.isEmpty) return trimmed;
    final lower = trimmed.toLowerCase();
    if (lower.startsWith('http://') || lower.startsWith('https://')) {
      return trimmed;
    }
    return 'https://$trimmed';
  }
}
