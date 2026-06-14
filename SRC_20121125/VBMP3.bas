Attribute VB_Name = "VBMP3"
Option Explicit

'VBMP3.BAS
'VisualBasic用 MP3操作DLL 関数宣言ファイル
Declare Function vbmp3_encodeOpen Lib "VBMP3.dll" (ByVal pszWaveName As String, pWaveForm As WAVE_FORM) As Boolean
Declare Function vbmp3_encodeStart Lib "VBMP3.dll" (ByVal pszMp3Name As String) As Boolean
Declare Function vbmp3_encodeStop Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_getEncodeState Lib "VBMP3.dll" (readSize As Long, encodeSize As Long) As Long
Declare Function vbmp3_getPlaySamples Lib "VBMP3.dll" () As Long
Declare Function vbmp3_getTotalSamples Lib "VBMP3.dll" () As Long
Declare Function vbmp3_setPlaySamples Lib "VBMP3.dll" (ByVal samples As Long) As Boolean
Declare Function vbmp3_getPlayFlames Lib "VBMP3.dll" () As Long
Declare Function vbmp3_setPlayFlames Lib "VBMP3.dll" (ByVal flames As Long) As Boolean
Declare Function vbmp3_setLyricsFile Lib "VBMP3.dll" (ByVal pszLyricsName As String) As Boolean
Declare Function vbmp3_getLyrics Lib "VBMP3.dll" (pLyricsInfo As LYRICS_INFO) As Boolean
Declare Sub vbmp3_getSpectrum Lib "VBMP3.dll" (pSpecL As Long, pSpecR As Long)
Declare Sub vbmp3_getWave Lib "VBMP3.dll" (pWaveL As Long, pWaveR As Long)
Declare Function vbmp3_decodeWave Lib "VBMP3.dll" (ByVal pszWaveName As String) As Boolean
Declare Function vbmp3_startCallback Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_stopCallback Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_callback Lib "VBMP3.dll" (ByVal pProc As Long) As Boolean
Declare Function vbmp3_getFileType Lib "VBMP3.dll" (ByVal pszName As String) As Long
Declare Function vbmp3_cutMacBinary Lib "VBMP3.dll" (ByVal pszName As String) As Boolean
Declare Function vbmp3_setListInfo Lib "VBMP3.dll" (ByVal pszName As String, pListInfo As LIST_INFO) As Boolean
Declare Function vbmp3_changeRmp Lib "VBMP3.dll" (ByVal pszName As String) As Boolean
Declare Function vbmp3_changeMp3 Lib "VBMP3.dll" (ByVal pszName As String) As Boolean
Declare Function vbmp3_changeWav Lib "VBMP3.dll" (ByVal pszName As String) As Boolean
Declare Function vbmp3_getListInfo Lib "VBMP3.dll" (pListInfo As LIST_INFO) As Boolean
Declare Function vbmp3_getFileTagInfo Lib "VBMP3.dll" (ByVal pszName As String, pTagInfo As TAG_INFO) As Boolean
Declare Function vbmp3_getFileInfo Lib "VBMP3.dll" (ByVal pszName As String, pTagInfo As TAG_INFO, pMpegTagInfo As MPEG_INFO) As Boolean
Declare Function vbmp3_getFileInfo2 Lib "VBMP3.dll" (ByVal pszName As String, pTagInfo As TAG_INFO, pMpegTagInfo As MPEG_INFO, pListInfo As LIST_INFO) As Boolean
Declare Function vbmp3_debug Lib "VBMP3.dll" () As Long
Declare Function vbmp3_getGenre Lib "VBMP3.dll" (pTagInfo As TAG_INFO) As Boolean
Declare Function vbmp3_getWinampPlayMs Lib "VBMP3.dll" () As Long
Declare Function vbmp3_getWinampTotalSec Lib "VBMP3.dll" () As Long
Declare Function vbmp3_getPlayBitRate Lib "VBMP3.dll" () As Long
Declare Function vbmp3_getLastErrorNo Lib "VBMP3.dll" () As Long
Declare Function vbmp3_setVbmp3Option Lib "VBMP3.dll" (pVbmp3Option As VBMP3_OPTION) As Long
Declare Sub vbmp3_getVbmp3Option Lib "VBMP3.dll" (pVbmp3Option As VBMP3_OPTION)
Declare Function vbmp3_setDecodeOption Lib "VBMP3.dll" (pDecOption As DEC_OPTION) As Long
Declare Sub vbmp3_getDecodeOption Lib "VBMP3.dll" (pDecOption As DEC_OPTION)
Declare Sub vbmp3_setEqualizer Lib "VBMP3.dll" (pTable As Long)
Declare Function vbmp3_getVersion Lib "VBMP3.dll" () As Long
Declare Function vbmp3_getTagInfo Lib "VBMP3.dll" (pTagInfo As TAG_INFO) As Boolean
Declare Function vbmp3_setTagInfo Lib "VBMP3.dll" (ByVal pszName As String, pTagInfo As TAG_INFO, Optional ByVal tagSet As Long = 0, Optional ByVal tagAdd As Long = 0) As Boolean
Declare Function vbmp3_getMpegInfo Lib "VBMP3.dll" (pMpegTagInfo As MPEG_INFO) As Boolean
Declare Function vbmp3_init Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_free Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_setStepPitch Lib "VBMP3.dll" (ByVal pitch As Long, Optional ByVal frames As Long = 5) As Boolean
Declare Function vbmp3_getStepPitch Lib "VBMP3.dll" () As Long
Declare Function vbmp3_reload Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_setPitch Lib "VBMP3.dll" (ByVal pitch As Long) As Boolean
Declare Function vbmp3_getPitch Lib "VBMP3.dll" () As Long
Declare Function vbmp3_open Lib "VBMP3.dll" (ByVal pszName As String, pInfo As InputInfo) As Boolean
Declare Function vbmp3_close Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_getState Lib "VBMP3.dll" (sec As Long) As Long
Declare Function vbmp3_play Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_stop Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_pause Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_restart Lib "VBMP3.dll" () As Boolean
Declare Function vbmp3_seek Lib "VBMP3.dll" (ByVal sec As Long) As Boolean
Declare Function vbmp3_setVolume Lib "VBMP3.dll" (ByVal lVol As Long, ByVal rVol As Long) As Boolean
Declare Function vbmp3_getVolume Lib "VBMP3.dll" (lVol As Long, rVol As Long) As Boolean
Declare Sub vbmp3_setFadeIn Lib "VBMP3.dll" (ByVal fin As Long)
Declare Sub vbmp3_setFadeOut Lib "VBMP3.dll" (ByVal fout As Long)
Declare Sub vbmp3_fadeOut Lib "VBMP3.dll" ()

'未使用
Declare Sub vbmp3_startAnalyzeThread Lib "VBMP3.dll" ()
Declare Sub vbmp3_stopAnalyzeThread Lib "VBMP3.dll" ()
Declare Sub vbmp3_startAnalyze Lib "VBMP3.dll" ()
Declare Sub vbmp3_stopAnalyze Lib "VBMP3.dll" ()
Declare Function vbmp3_getWaveData Lib "VBMP3.dll" (pWaveData As WAVE_DATA) As Boolean

Public Type InputInfo
    szTrackName As String * 128     '曲名
    szArtistName As String * 128    'ｱｰﾃｨｽﾄ名
    channels As Long                'ﾁｬﾝﾈﾙ数
    bitRate As Long                 'ﾋﾞｯﾄﾚｰﾄ(kbit/s)
    samplingRate As Long            'ｻﾝﾌﾟﾙﾚｰﾄ(Hz)
    totalSec As Long                '演奏時間(s)
End Type

Public Type TAG_INFO
    szTrackName As String * 128     '曲名
    szArtistName As String * 128    'ｱｰﾃｨｽﾄ名
    szAlbumName As String * 128     'ｱﾙﾊﾞﾑ名
    szYear As String * 5            'ﾘﾘｰｽ年号
    szComment As String * 128       'ｺﾒﾝﾄ
    genre As Long                   'ｼﾞｬﾝﾙ
    szGenreName As String * 128     'ｼﾞｬﾝﾙ名
End Type

Public Type MPEG_INFO
    version As Long                 'ﾊﾞｰｼﾞｮﾝ        1:MPEG-1, 2:MPEG-2, 3:MPEG-2.5
    layer As Long                   'ﾚｲﾔ            1:Layer1. 2:Layer2, 3:Layer3
    crcDisable As Long              'ｴﾗｰ保護        0:無効, 1:有効
    extension As Long               'ｴｸｽﾃﾝｼｮﾝ       0:なし, 1:ﾌﾟﾗｲﾍﾞｰﾄ
    Mode As Long                    'ﾁｬﾝﾈﾙﾓｰﾄﾞ      0:Stereo, 1:Joint stereo, 3:Dual channel, 4:Mono
    copyright As Long               '著作権         0:著作権保護あり, 1:著作権保護なし
    original As Long                'ｵﾘｼﾞﾅﾙ         0:ｺﾋﾟｰ, 1:ｵﾘｼﾞﾅﾙ
    emphasis As Long                'ｴﾝﾌｫｼｽ         0:None, 1:50/15ms, 2:Reserved, 3:CCITT j.17
    
    channels As Long                'ﾁｬﾝﾈﾙ数
    bitRate As Long                 'ﾋﾞｯﾄﾚｰﾄ(kbit/s)(0 なら VBR形式)
    samplingRate As Long            'ｻﾝﾌﾟﾙﾚｰﾄ(Hz)
    fileSize As Long                'ﾌｧｲﾙｻｲｽﾞ(Byte)
    flames As Long                  'ﾌﾚｰﾑ数
    totalSec As Long                '演奏時間(s)
End Type

Public Type DEC_OPTION
    reduction As Long               'サンプリング 0:1/1 1:1/2 2:1/4 [Default = 0]
    convert As Long                 'チャンネル 0:ステレオ 1:モノラル[Default = 0]
    freqLimit As Long               '周波数[Default = 24000]
End Type

Public Type VBMP3_OPTION
    inputBlock As Long              '入力フレーム数[Default = 40]
    outputBlock As Long             '出力フレーム数[Default = 30]
    inputSleep As Long              '入力直後のｽﾘｰﾌﾟ時間(ﾐﾘ秒)[Default = 5]
    outputSleep As Long             '出力直後のｽﾘｰﾌﾟ時間(ﾐﾘ秒)[Default = 0]
End Type

Public Type LIST_INFO
    INAM As String * 128            '曲名
    IART As String * 128            'アーティスト名
    IPRD As String * 128            '製品名
    ICMT As String * 128            'コメント文字列
    ICRD As String * 128            'リリース年号
    IGNR As String * 128            'ジャンル名
    ICOP As String * 128            '著作権
    IENG As String * 128            'エンジニア
    ISRC As String * 128            'ソース
    ISFT As String * 128            'ソフトウェア
    IKEY As String * 128            'キーワード
    ITCH As String * 128            '技術者
    ILYC As String * 128            '歌詞
    ICMS As String * 128            'コミッション
End Type

Public Type LYRICS_INFO
    sec As Long
    LyricsNext2 As String * 128
    LyricsNext1 As String * 128
    LyricsCurrent As String * 128
    LyricsPrev1 As String * 128
    LyricsPrev2 As String * 128
End Type

Public Type WAVE_DATA
    channels As Long
    bitsPerSample As Long
    Left As Long
    Right As Long
End Type

Public Type WAVE_FORM
    channels As Long
    bitsPerSample As Long
    samplingRate As Long
    dataSize As Long
End Type

'ファイルタイプ定数
Public Const FT_NOMAL = 0
Public Const FT_WAVE = 1
Public Const FT_RMP = 2
Public Const FT_ID3V2 = 4
Public Const FT_MAC = 8
Public Const FT_ID3V1 = 16

'コールバック関数用
Public Const MSG_ERROR = 0
Public Const MSG_STOPING = 1
Public Const MSG_PLAYING = 2
Public Const MSG_PAUSING = 3
Public Const MSG_PLAYDONE = 4

Enum vbmp3_errNo
    ERR_MP3_FILE_OPEN = 1
    ERR_MP3_FILE_NOT_OPEN = 2
    ERR_MP3_FILE_READ = 3
    ERR_MP3_FILE_WRITE = 4
    ERR_WAV_FILE_OPEN = 5
    ERR_WAV_FORMAT = 6
    ERR_ENCODE_FILE_OPEN = 7
    ERR_LYRICS_FILE_OPEN = 8
    ERR_LYRICS_NON_DATA = 9
    ERR_FRAME_HEADER_NOT_FOUND = 10
    ERR_FRAME_HEADER_READ = 11
    ERR_STATE_STOP = 12
    ERR_NOT_STATE_STOP = 13
    ERR_NOT_STATE_PLAY = 14
    ERR_STATE_NON_ENCODE = 15
    ERR_PLAY = 16
    ERR_STOP = 17
    ERR_INVALID_VALUE = 18
    ERR_MALLOC = 19
    ERR_NON_RIFF = 20
    ERR_RIFF = 21
    ERR_NOT_MP3 = 22
    ERR_MAC_BIN = 23
    ERR_UNKNOWN_FILE = 24
    ERR_OPEN_OUT_DEVICE = 25
    ERR_DECODE = 26
    ERR_DECODE_THREAD = 27
    ERR_ENCODE_THREAD = 28
    ERR_CREATE_EVENT = 29
    ERR_CODEC_NOT_FOUND = 30
    ERR_WAVE_TABLE_NOT_FOUND = 31
    ERR_ACM_OPEN = 32
End Enum


'---------------------------------------------------------
'関数：Function NTrim()
'機能：\0 以降の文字列削除
'引数：Word  : 変換元文字列
'戻り値：変換後文字列
'---------------------------------------------------------
Function NTrim(Word As String) As String
    If InStr(Word, Chr(0)) > 0 Then
        NTrim = Left(Word, InStr(Word, Chr(0)) - 1)
    Else
        NTrim = Word
    End If
End Function

