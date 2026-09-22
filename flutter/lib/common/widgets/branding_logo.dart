import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart';

/// Company logo clipped to rounded corners of the laid-out image,
/// independent of the source bitmap size.
class BrandingLogoImage extends StatefulWidget {
  const BrandingLogoImage({
    Key? key,
    required this.path,
    required this.cacheKey,
    this.maxWidth = double.infinity,
    this.maxHeight = double.infinity,
    this.fit = BoxFit.contain,
    this.alignment = Alignment.center,
  }) : super(key: key);

  final String path;
  final String cacheKey;
  final double maxWidth;
  final double maxHeight;
  final BoxFit fit;
  final Alignment alignment;

  @override
  State<BrandingLogoImage> createState() => _BrandingLogoImageState();
}

class _BrandingLogoImageState extends State<BrandingLogoImage> {
  ImageStream? _stream;
  ImageStreamListener? _listener;
  Size? _pixelSize;
  bool _failed = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _subscribe();
  }

  @override
  void didUpdateWidget(BrandingLogoImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.path != widget.path || oldWidget.cacheKey != widget.cacheKey) {
      _pixelSize = null;
      _failed = false;
      _subscribe();
    }
  }

  @override
  void dispose() {
    _unsubscribe();
    super.dispose();
  }

  void _subscribe() {
    _unsubscribe();
    final provider = FileImage(File(widget.path));
    _stream = provider.resolve(createLocalImageConfiguration(context));
    _listener = ImageStreamListener(_handleImage, onError: _handleError);
    _stream!.addListener(_listener!);
  }

  void _unsubscribe() {
    if (_stream != null && _listener != null) {
      _stream!.removeListener(_listener!);
    }
    _stream = null;
    _listener = null;
  }

  void _handleImage(ImageInfo info, bool synchronousCall) {
    final size = Size(info.image.width.toDouble(), info.image.height.toDouble());
    if (_pixelSize == size && !_failed) return;
    if (synchronousCall) {
      _pixelSize = size;
      _failed = false;
      return;
    }
    if (!mounted) return;
    setState(() {
      _pixelSize = size;
      _failed = false;
    });
  }

  void _handleError(Object exception, StackTrace? stackTrace) {
    if (_failed) return;
    _failed = true;
    _pixelSize = null;
    if (!mounted) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) setState(() {});
    });
  }

  Size _displaySize(BoxConstraints constraints, Size pixels) {
    var maxW = constraints.maxWidth;
    var maxH = constraints.maxHeight;
    if (widget.maxWidth.isFinite) {
      maxW = maxW.isFinite ? math.min(maxW, widget.maxWidth) : widget.maxWidth;
    }
    if (widget.maxHeight.isFinite) {
      maxH = maxH.isFinite ? math.min(maxH, widget.maxHeight) : widget.maxHeight;
    }
    if (!maxW.isFinite) maxW = pixels.width;
    if (!maxH.isFinite) maxH = pixels.height;
    if (pixels.width <= 0 || pixels.height <= 0 || maxW <= 0 || maxH <= 0) {
      return Size.zero;
    }
    switch (widget.fit) {
      case BoxFit.fitWidth:
        final h = maxW * pixels.height / pixels.width;
        return Size(maxW, math.min(h, maxH));
      case BoxFit.fitHeight:
        final w = maxH * pixels.width / pixels.height;
        return Size(math.min(w, maxW), maxH);
      case BoxFit.fill:
        return Size(maxW, maxH);
      case BoxFit.cover:
        final scale = math.max(maxW / pixels.width, maxH / pixels.height);
        return Size(pixels.width * scale, pixels.height * scale);
      case BoxFit.none:
        return Size(math.min(pixels.width, maxW), math.min(pixels.height, maxH));
      case BoxFit.scaleDown:
        if (pixels.width <= maxW && pixels.height <= maxH) return pixels;
        return _contain(pixels, maxW, maxH);
      case BoxFit.contain:
        return _contain(pixels, maxW, maxH);
    }
  }

  Size _contain(Size pixels, double maxW, double maxH) {
    final scale = math.min(maxW / pixels.width, maxH / pixels.height);
    return Size(pixels.width * scale, pixels.height * scale);
  }

  @override
  Widget build(BuildContext context) {
    final pixels = _pixelSize;
    if (_failed || pixels == null) {
      return const SizedBox.shrink();
    }
    return LayoutBuilder(
      builder: (context, constraints) {
        final dest = _displaySize(constraints, pixels);
        if (dest.width <= 0 || dest.height <= 0) {
          return const SizedBox.shrink();
        }
        final shorter = math.min(dest.width, dest.height);
        final radius = (shorter * 0.12).clamp(6.0, 24.0);
        return ClipRRect(
          borderRadius: BorderRadius.circular(radius),
          clipBehavior: Clip.antiAlias,
          child: Image.file(
            File(widget.path),
            key: ValueKey(widget.cacheKey),
            width: dest.width,
            height: dest.height,
            fit: widget.fit,
            alignment: widget.alignment,
            gaplessPlayback: false,
            errorBuilder: (ctx, error, stackTrace) => const SizedBox.shrink(),
          ),
        );
      },
    );
  }
}
