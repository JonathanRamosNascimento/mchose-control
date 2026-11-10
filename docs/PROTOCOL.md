# Protocolo do receptor MCHOSE (C-Media `291d:385d`)

Documento da engenharia reversa feita para o **MCHOSE Control**. Vale para os headsets
que usam o mesmo receptor 2.4 GHz: **V9 PRO, V9, G9 PRO e S9 PRO** (o modelo é
identificado pela string de produto USB, ex.: `MCHOSE V9 PRO`).

## Fontes

| Fonte | O que forneceu |
|---|---|
| Descritores USB / HID do dispositivo (`lsusb -v`, `/sys/class/hidraw/*/device/report_descriptor`) | Interfaces, endpoints e os *report IDs* existentes |
| **M HUB Web** (`www.mchose.com.cn`, app Vue/Vite com WebHID) | Implementação JavaScript do perfil `BLK`: bytes de comando, parsing das respostas, regras de firmware |
| **MCHOSE HUB** para Windows (`MCHOSE_HUB_installer.zip` → NSIS → `app-64.7z` → Electron) | Pacote de driver `libs/S9_PRO_14429`: driver de áudio `BLK2025GHS.sys` + APO Xear `CmApoIce.dll` |

Nada foi executado do software oficial: os pacotes foram apenas extraídos e lidos.

## Topologia USB

```
291d:385d  "C-Media Electronics Inc" / "MCHOSE V9 PRO"   USB 2.0 full speed
├── if 0  HID      EP 0x82 IN (int, 64 B)  · EP 0x03 OUT (int, 64 B)   ← protocolo vendor
├── if 1  Audio Control (UAC1)   Feature Unit 9 (mute+volume L/R) · Feature Unit 5 (mic)
├── if 2  Audio Streaming OUT    2 ch · 24 bit · 48 kHz
└── if 3  Audio Streaming IN     1 ch · 24/16 bit · 48 kHz
```

No Windows o driver proprietário só se associa a `USB\VID_291D&PID_385D&MI_01` (áudio).
O HID continua com o driver genérico do sistema — por isso o mesmo protocolo funciona
pelo `hidraw` no Linux.

## Report IDs (interface 0)

| ID | Usage page | Tipo | Payload | Uso |
|---|---|---|---|---|
| `0x55` | `0xFF90` | Input + Output | 63 B | **Comandos, respostas e notificações** |
| `0xAA` | `0xFF22` | Feature | 63 B | **Versão de firmware** |
| `0x41` | `0xFF82` | Input + Output | 63 B | Dados de atualização de firmware (OTA) — **não usado** |
| `0xED` | `0xFF21` | In/Out 1 B · Feature 15 B | — | Canal C-Media padrão (não usado pelo HUB) |
| `0x06` | Consumer | Input | 2 B | Vol+/Vol−/Mute/Play… (tratado pelo kernel) |
| `0x07` | Telephony | Input + LEDs | — | Hook/mute de telefonia (tratado pelo kernel) |

Os frames abaixo são escritos como no `hidraw`: **byte 0 = report ID**, seguido do payload,
completado com zeros até 64 bytes (o Chrome faz o mesmo preenchimento no Windows).

## Comandos (report `0x55`)

### Modo de som

| Ação | Frame enviado | Resposta (input `0x55`) |
|---|---|---|
| Ler | `55 11 00 …` | `55 11 <modo> …` |
| Definir | `55 21 01 <modo> 00 …` | (nenhuma; o app relê com `55 11`) |

`<modo>`: `00` = Game 1 · `01` = Game 2 · `02` = Music. O headset também envia `55 11 <modo>`
espontaneamente quando o botão de modo do fone é pressionado.

### Bateria

| Ação | Frame enviado | Resposta |
|---|---|---|
| Ler | `55 65 01 00 …` | `55 65 <nível> <status> …` |

* `<nível>`: porcentagem 0–100.
* `<status>`: `02` descarregando · `03` carregando · `04` carga completa · `1A` **headset desligado/em repouso** (receptor conectado).

Leituras de bateria também chegam sem solicitação.

## Firmware (feature report `0xAA`)

1. `SET_FEATURE`: `AA 01 <alvo> 00 …` — `<alvo>`: `00` headset · `01` receptor
2. aguardar **300 ms**
3. `GET_FEATURE` do report `0xAA` → `AA 01 b0 b1 b2 b3 …`

A versão é a concatenação **decimal** dos 4 bytes: `00 00 02 09` → `"0029"` → 29.

### Regra "P1 / P8"

O HUB só usa **bateria e modo de som** quando o firmware do receptor é novo o bastante
(perfil "P8"): V9 PRO ≥ **8**, G9 PRO ≥ **6**. Receptores mais antigos ("P1") respondem
apenas à consulta de firmware. Versões de referência no HUB: V9 PRO → receptor `0008`,
headset `0029`.

## Sequência de inicialização usada pelo HUB

1. Ler firmware do receptor.
2. Se P8: ler bateria; se o headset **não** estiver em repouso, ler firmware do headset.
3. Ler modo de som.
4. Escutar notificações em `0x55` (bateria e modo).

## EQ, surround, microfone: onde ficam?

Nenhum desses recursos passa pelo HID. No HUB eles são chamados via
`desktop.invoke("AudioSDKCall", …)` (`setEQ_Slider`, `setXearSurroundEnable`,
`setEnable_VIRTUALBASS_LFX`, `setEnable_MAGICVOICE`, `setAINREnable`…), que vai para o SDK
nativo do **Xear** da C-Media. O pacote de driver traz o `CmApoIce.dll`, registrado como
*Audio Processing Object* de reprodução (SFX/MFX/EFX) e gravação no motor de áudio do
Windows — strings como `XearONE`, `VirtualBassApp.cpp`, `AudioBrilliantApp.cpp`,
`Xear Surround Headphone` confirmam. Ou seja: **todo o processamento é feito no PC**.

No Linux o MCHOSE Control faz o mesmo papel com o PipeWire:

| Xear (Windows) | MCHOSE Control (Linux) |
|---|---|
| EQ 10 bandas (`eq_band_N_gain_db`) | `bq_peaking` × 10 por canal (31 Hz…16 kHz, Q 1,41, ±12 dB) |
| Virtual Bass | `bq_lowshelf` (nível + frequência de corte) |
| Audio Brilliant | `bq_highshelf` |
| Surround Headphone | Crossfeed (passa-baixa 700 Hz + atraso 0,3 ms entre canais) |
| AINR / Voice Clarity / Mic leveling | `module-echo-cancel` com WebRTC (supressão de ruído, AGC, passa-alta, transientes) |

## Escopo de segurança

O MCHOSE Control **nunca** escreve nos reports `0x41` (OTA) nem `0xED`. Só são usados
os comandos de leitura e o comando de modo de som, exatamente como no app oficial.
