import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_hbb/common/widgets/setting_widgets.dart';
import 'package:flutter_hbb/common/widgets/toolbar.dart';
import 'package:get/get.dart';

import '../../common.dart';
import '../../models/platform_model.dart';

void _showSuccess() {
  showToast(translate("Successful"));
}

/// "Connect with a code": server address + 6-character pairing code.
///
/// The client needs only these two values; the server answers with every endpoint the
/// manual dialog would ask for. Deliberately not a QR substitute UI — the code path is
/// for people typing on a keyboard, the QR path stays in the mobile scan page.
void showPairingDialog(OverlayDialogManager dialogManager,
    void Function(VoidCallback)? onPaired) async {
  final hostCtrl = TextEditingController();
  final codeCtrl = TextEditingController();
  var busy = false;
  var statusText = '';
  var statusIsError = false;

  dialogManager.show((setState, close, context) {
    Future<void> submit() async {
      final host = hostCtrl.text.trim();
      final code = codeCtrl.text.trim();
      if (host.isEmpty || code.isEmpty) {
        setState(() {
          statusIsError = true;
          statusText = translate('Enter the server address and the code');
        });
        return;
      }
      setState(() {
        busy = true;
        statusText = translate('Connecting...');
        statusIsError = false;
      });
      try {
        await bind.mainSetCommon(key: 'pairing-input', value: '$host\n$code');
        final raw = await bind.mainGetCommon(key: 'pairing-redeem');
        final result = jsonDecode(raw);
        if (result is! Map<String, dynamic>) {
          throw 'invalid response';
        }
        if (result['ok'] != true) {
          final key = result['error_key'] as String? ?? '';
          setState(() {
            busy = false;
            statusIsError = true;
            statusText = key.isEmpty
                ? translate('Could not pair with the server')
                : translate(_pairingErrorText(key));
          });
          return;
        }
        final serverName = (result['server_name'] as String?) ?? '';
        final enrollment = (result['enrollment_status'] as String?) ?? '';
        setState(() {
          busy = false;
          statusIsError = false;
          statusText = serverName.isEmpty
              ? translate('Connected')
              : '${translate('Connected')}: $serverName'
                  '${enrollment.isEmpty ? '' : ' ($enrollment)'}';
        });
        if (result['insecure'] == true) {
          showToast(translate('Connected over plain HTTP. Enable HTTPS on the server.'));
        }
        onPaired?.call(() {});
        // Keep the dialog open briefly so the operator sees the result, then close.
        await Future.delayed(const Duration(milliseconds: 900));
        close();
      } catch (e) {
        debugPrint('pairing failed: $e');
        setState(() {
          busy = false;
          statusIsError = true;
          statusText = translate('Could not pair with the server');
        });
      }
    }

    Widget field(String label, TextEditingController controller,
        {bool autofocus = false,
        int? maxLength,
        TextCapitalization capitalization = TextCapitalization.none}) {
      return Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: TextField(
          controller: controller,
          autofocus: autofocus,
          maxLength: maxLength,
          enabled: !busy,
          textCapitalization: capitalization,
          autocorrect: false,
          enableSuggestions: false,
          decoration: InputDecoration(
            labelText: label,
            counterText: '',
            border: const OutlineInputBorder(),
          ),
          onSubmitted: (_) => submit(),
        ),
      );
    }

    return CustomAlertDialog(
      title: Text(translate('Connect with a code')),
      content: ConstrainedBox(
        constraints: const BoxConstraints(minWidth: 420),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              translate(
                  'Type the server address and the 6-character code shown in the BetterDesk console.'),
              style: const TextStyle(fontSize: 12),
            ),
            const SizedBox(height: 12),
            field(translate('Server address'), hostCtrl, autofocus: true),
            field(translate('Pairing code'), codeCtrl,
                maxLength: 6, capitalization: TextCapitalization.characters),
            if (statusText.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Row(
                  children: [
                    if (busy)
                      const Padding(
                        padding: EdgeInsets.only(right: 8),
                        child: SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        ),
                      ),
                    Expanded(
                      child: Text(
                        statusText,
                        style: TextStyle(
                          fontSize: 12,
                          color: statusIsError ? Colors.redAccent : null,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
      actions: [
        dialogButton('Cancel', onPressed: busy ? null : () => close(),
            isOutline: true),
        dialogButton('Connect', onPressed: busy ? null : () {
          submit();
        }),
      ],
    );
  });
}

/// Maps a Rust-side pairing error key to the display text.
String _pairingErrorText(String key) {
  switch (key) {
    case 'pairing-code-format':
      return 'The code must be 6 characters (letters and digits, no 0, 1, O, I, L, U)';
    case 'pairing-code-invalid':
      return 'The code is invalid or has expired';
    case 'pairing-host-required':
      return 'Enter the server address, for example desk.example.com';
    case 'pairing-host-mismatch':
      return 'The server answered with a different address. Check the address and try again.';
    case 'pairing-device-id-missing':
      return 'This device has no ID yet. Start the service and try again.';
    case 'pairing-server-unreachable':
    default:
      return 'Could not reach the server. Check the address and that HTTPS is enabled.';
  }
}

void setTemporaryPasswordLengthDialog(
    OverlayDialogManager dialogManager) async {
  List<String> lengths = ['6', '8', '10'];
  String length = await bind.mainGetOption(key: "temporary-password-length");
  var index = lengths.indexOf(length);
  if (index < 0) index = 0;
  length = lengths[index];
  dialogManager.show((setState, close, context) {
    setLength(newValue) {
      final oldValue = length;
      if (oldValue == newValue) return;
      setState(() {
        length = newValue;
      });
      bind.mainSetOption(key: "temporary-password-length", value: newValue);
      bind.mainUpdateTemporaryPassword();
      Future.delayed(Duration(milliseconds: 200), () {
        close();
        _showSuccess();
      });
    }

    return CustomAlertDialog(
      title: Text(translate("Set one-time password length")),
      content: Row(
          mainAxisAlignment: MainAxisAlignment.spaceEvenly,
          children: lengths
              .map(
                (value) => Row(
                  children: [
                    Text(value),
                    Radio(
                        value: value, groupValue: length, onChanged: setLength),
                  ],
                ),
              )
              .toList()),
    );
  }, backDismiss: true, clickMaskDismiss: true);
}

void showServerSettings(OverlayDialogManager dialogManager,
    void Function(VoidCallback) setState) async {
  Map<String, dynamic> options = {};
  try {
    options = jsonDecode(await bind.mainGetOptions());
  } catch (e) {
    print("Invalid server config: $e");
  }
  showServerSettingsWithValue(
      ServerConfig.fromOptions(options), dialogManager, setState);
}

void showServerSettingsWithValue(
    ServerConfig serverConfig,
    OverlayDialogManager dialogManager,
    void Function(VoidCallback)? upSetState) async {
  var isInProgress = false;
  final idCtrl = TextEditingController(text: serverConfig.idServer);
  final relayCtrl = TextEditingController(text: serverConfig.relayServer);
  final apiCtrl = TextEditingController(text: serverConfig.apiServer);
  final keyCtrl = TextEditingController(text: serverConfig.key);

  RxString idServerMsg = ''.obs;
  RxString relayServerMsg = ''.obs;
  RxString apiServerMsg = ''.obs;

  final controllers = [idCtrl, relayCtrl, apiCtrl, keyCtrl];
  final errMsgs = [
    idServerMsg,
    relayServerMsg,
    apiServerMsg,
  ];

  dialogManager.show((setState, close, context) {
    Future<bool> submit() async {
      setState(() {
        isInProgress = true;
      });
      bool ret = await setServerConfig(
          null,
          errMsgs,
          ServerConfig(
              idServer: idCtrl.text.trim(),
              relayServer: relayCtrl.text.trim(),
              apiServer: apiCtrl.text.trim(),
              key: keyCtrl.text.trim()));
      setState(() {
        isInProgress = false;
      });
      return ret;
    }

    Widget buildField(
        String label, TextEditingController controller, String errorMsg,
        {String? Function(String?)? validator, bool autofocus = false}) {
      if (isDesktop || isWeb) {
        return Row(
          children: [
            SizedBox(
              width: 120,
              child: Text(label),
            ),
            SizedBox(width: 8),
            Expanded(
              child: serverSettingsTextFormField(
                label: label,
                controller: controller,
                errorMsg: errorMsg,
                contentPadding:
                    EdgeInsets.symmetric(horizontal: 8, vertical: 12),
                showLabelText: false,
                validator: validator,
                autofocus: autofocus,
              ).workaroundFreezeLinuxMint(),
            ),
          ],
        );
      }

      return serverSettingsTextFormField(
        label: label,
        controller: controller,
        errorMsg: errorMsg,
        validator: validator,
      ).workaroundFreezeLinuxMint();
    }

    return CustomAlertDialog(
      title: Row(
        children: [
          Expanded(child: Text(translate('ID/Relay Server'))),
          ...ServerConfigImportExportWidgets(controllers, errMsgs),
        ],
      ),
      content: ConstrainedBox(
        constraints: const BoxConstraints(minWidth: 500),
        child: Form(
          child: Obx(() => Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  buildField(translate('ID Server'), idCtrl, idServerMsg.value,
                      autofocus: true),
                  SizedBox(height: 8),
                  if (!isIOS && !isWeb) ...[
                    buildField(translate('Relay Server'), relayCtrl,
                        relayServerMsg.value),
                    SizedBox(height: 8),
                  ],
                  buildField(
                    translate('API Server'),
                    apiCtrl,
                    apiServerMsg.value,
                    validator: (v) {
                      if (v != null && v.isNotEmpty) {
                        if (!(v.startsWith('http://') ||
                            v.startsWith("https://"))) {
                          return translate("invalid_http");
                        }
                      }
                      return null;
                    },
                  ),
                  SizedBox(height: 8),
                  buildField('Key', keyCtrl, ''),
                  if (isInProgress)
                    Padding(
                      padding: EdgeInsets.only(top: 8),
                      child: LinearProgressIndicator(),
                    ),
                ],
              )),
        ),
      ),
      actions: [
        dialogButton('Cancel', onPressed: () {
          close();
        }, isOutline: true),
        dialogButton(
          'OK',
          onPressed: () async {
            if (await submit()) {
              close();
              showToast(translate('Successful'));
              upSetState?.call(() {});
            } else {
              showToast(translate('Failed'));
            }
          },
        ),
      ],
    );
  });
}

TextFormField serverSettingsTextFormField({
  required String label,
  required TextEditingController controller,
  required String errorMsg,
  String? Function(String?)? validator,
  bool autofocus = false,
  bool showLabelText = true,
  EdgeInsetsGeometry? contentPadding,
}) {
  return TextFormField(
    controller: controller,
    decoration: InputDecoration(
      labelText: showLabelText ? label : null,
      errorText: errorMsg.isEmpty ? null : errorMsg,
      contentPadding: contentPadding,
    ),
    validator: validator,
    autofocus: autofocus,
    keyboardType: TextInputType.visiblePassword,
    textCapitalization: TextCapitalization.none,
    autocorrect: false,
    enableSuggestions: false,
    smartDashesType: SmartDashesType.disabled,
    smartQuotesType: SmartQuotesType.disabled,
    enableIMEPersonalizedLearning: false,
    spellCheckConfiguration: const SpellCheckConfiguration.disabled(),
  );
}

void setPrivacyModeDialog(
  OverlayDialogManager dialogManager,
  List<TToggleMenu> privacyModeList,
  RxString privacyModeState,
) async {
  dialogManager.dismissAll();
  dialogManager.show((setState, close, context) {
    return CustomAlertDialog(
      title: Text(translate('Privacy mode')),
      content: Column(
          mainAxisAlignment: MainAxisAlignment.spaceEvenly,
          children: privacyModeList
              .map((value) => CheckboxListTile(
                    contentPadding: EdgeInsets.zero,
                    visualDensity: VisualDensity.compact,
                    title: value.child,
                    value: value.value,
                    onChanged: value.onChanged,
                  ))
              .toList()),
    );
  }, backDismiss: true, clickMaskDismiss: true);
}
