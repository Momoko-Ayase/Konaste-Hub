//! Interface strings. Five locales; a missing entry falls back to English.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    En,
    Ja,
    ZhCn,
    ZhTw,
    Ko,
}

impl Locale {
    pub const ALL: [Locale; 5] = [Locale::En, Locale::Ja, Locale::ZhCn, Locale::ZhTw, Locale::Ko];
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Ja => "ja",
            Locale::ZhCn => "zh-CN",
            Locale::ZhTw => "zh-TW",
            Locale::Ko => "ko",
        }
    }
    pub fn from_code(code: &str) -> Self {
        Self::ALL.into_iter().find(|l| l.code() == code).unwrap_or(Locale::En)
    }
    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Ja => "日本語",
            Locale::ZhCn => "简体中文",
            Locale::ZhTw => "繁體中文",
            Locale::Ko => "한국어",
        }
    }
}

macro_rules! strings {
    ($($key:ident => [$en:expr, $ja:expr, $zhcn:expr, $zhtw:expr, $ko:expr],)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[allow(non_camel_case_types)]
        pub enum K { $($key,)* }
        pub fn t(locale: Locale, key: K) -> &'static str {
            match key {
                $(K::$key => match locale {
                    Locale::En => $en,
                    Locale::Ja => $ja,
                    Locale::ZhCn => $zhcn,
                    Locale::ZhTw => $zhtw,
                    Locale::Ko => $ko,
                },)*
            }
        }
    };
}

strings! {
    play => ["Play", "ゲーム開始", "开始游戏", "開始遊戲", "게임 시작"],
    update => ["Update", "更新", "更新", "更新", "업데이트"],
    retry => ["Retry", "再試行", "重试", "重試", "다시 시도"],
    install => ["Install", "インストール", "安装", "安裝", "설치"],
    starting => ["Starting", "起動中", "正在启动", "正在啟動", "시작 중"],
    running => ["Running", "プレイ中", "运行中", "執行中", "실행 중"],
    checking => ["Checking for updates", "更新を確認中", "正在检查更新", "正在檢查更新", "업데이트 확인 중"],
    queued => ["Queued", "待機中", "排队中", "排隊中", "대기 중"],
    preparing => ["Checking local files", "ローカルファイルを確認中", "正在检查本地文件", "正在檢查本機檔案", "로컬 파일 확인 중"],
    downloading => ["Downloading", "ダウンロード中", "正在下载", "正在下載", "다운로드 중"],
    paused => ["Paused", "一時停止中", "已暂停", "已暫停", "일시 정지됨"],
    verifying => ["Verifying files", "ファイルを検証中", "正在校验文件", "正在驗證檔案", "파일 확인 중"],
    installing => ["Installing", "インストール中", "正在安装", "正在安裝", "설치 중"],
    recovering => ["Recovering", "復旧中", "正在恢复", "正在復原", "복구 중"],
    finalizing => ["Checking installation", "インストールを確認中", "正在检查安装", "正在檢查安裝", "설치 확인 중"],
    opening_settings => ["Opening game settings", "ゲーム設定を開いています", "正在打开游戏设置", "正在開啟遊戲設定", "게임 설정 여는 중"],
    configuring => ["Game settings open", "ゲーム設定中", "游戏设置已打开", "遊戲設定已開啟", "게임 설정 열림"],
    maintenance => ["Maintenance", "メンテナンス中", "维护中", "維護中", "점검 중"],
    maintenance_until => ["Online services are under maintenance until {} JST.", "オンラインサービスは {}（日本時間）までメンテナンス中です。", "在线服务维护中，预计 {}（日本时间）结束。", "線上服務維護中，預計 {}（日本時間）結束。", "온라인 서비스 점검 중입니다. {}(일본 시간) 종료 예정."],
    maintenance_service => ["Online services are under maintenance.", "オンラインサービスはメンテナンス中です。", "在线服务维护中。", "線上服務維護中。", "온라인 서비스 점검 중입니다."],
    update_size => ["Update available", "更新があります", "有可用更新", "有可用更新", "업데이트 있음"],
    news => ["News", "ニュース", "新闻", "新聞", "뉴스"],
    details => ["Details", "詳細", "详情", "詳細", "상세"],
    more => ["More on the official site", "公式サイトで見る", "在官网查看更多", "在官網查看更多", "공식 사이트에서 더 보기"],
    no_news => ["Could not load news.", "ニュースを読み込めませんでした。", "无法加载新闻。", "無法載入新聞。", "뉴스를 불러올 수 없습니다."],
    version => ["Version", "バージョン", "版本", "版本", "버전"],
    content => ["Content", "コンテンツ", "内容", "內容", "콘텐츠"],
    location => ["Location", "インストール先", "位置", "位置", "위치"],
    open_folder => ["Open folder", "フォルダーを開く", "打开文件夹", "開啟資料夾", "폴더 열기"],
    verify => ["Verify files", "ファイルを検証", "校验文件", "驗證檔案", "파일 확인"],
    game_settings => ["Game settings", "ゲーム設定", "游戏设置", "遊戲設定", "게임 설정"],
    official_site => ["Official site", "公式サイト", "官方网站", "官方網站", "공식 사이트"],
    remove => ["Remove from library", "ライブラリから外す", "从库中移除", "從收藏庫移除", "라이브러리에서 제거"],
    instrument => ["Instrument", "楽器", "乐器", "樂器", "악기"],
    guitar => ["GuitarFreaks", "ギターフリークス", "GuitarFreaks", "GuitarFreaks", "GuitarFreaks"],
    drum => ["DrumMania", "ドラムマニア", "DrumMania", "DrumMania", "DrumMania"],
    add_game => ["Add game", "ゲームを追加", "添加游戏", "新增遊戲", "게임 추가"],
    locate => ["Locate installed", "インストール済みを指定", "选择已安装位置", "選擇已安裝位置", "설치 위치 지정"],
    not_supported => ["Not supported yet", "未対応", "暂不支持", "尚未支援", "아직 지원하지 않음"],
    install_title => ["Install {}", "{} をインストール", "安装 {}", "安裝 {}", "{} 설치"],
    install_folder => ["Install folder", "インストール先", "安装位置", "安裝位置", "설치 폴더"],
    browse => ["Browse", "参照", "浏览", "瀏覽", "찾아보기"],
    free_space => ["Free space", "空き容量", "可用空间", "可用空間", "여유 공간"],
    cancel => ["Cancel", "キャンセル", "取消", "取消", "취소"],
    terms_title => ["Konaste Terms of Use", "コナステ 利用規約", "Konaste 使用条款", "Konaste 使用條款", "Konaste 이용 약관"],
    terms_body => ["Accept the terms to play {}.", "{} をプレイするには利用規約への同意が必要です。", "游玩 {} 需要同意使用条款。", "遊玩 {} 需要同意使用條款。", "{}을(를) 플레이하려면 이용 약관에 동의해야 합니다."],
    terms_accept => ["Accept", "同意する", "同意", "同意", "동의"],
    still_running_title => ["A previous game may still be running", "前のゲームが起動中の可能性があります", "之前的游戏可能仍在运行", "之前的遊戲可能仍在執行", "이전 게임이 아직 실행 중일 수 있습니다"],
    still_running_body => ["One account can run one game at a time. Starting now closes the previous game.", "同一アカウントで同時に複数のゲームを起動することはできません。新しく起動すると、前に起動したゲームは強制終了されます。", "同一账号不能同时运行多个游戏。现在启动会强制关闭之前的游戏。", "同一帳號無法同時執行多個遊戲。現在啟動會強制關閉之前的遊戲。", "한 계정으로 여러 게임을 동시에 실행할 수 없습니다. 지금 시작하면 이전 게임이 강제 종료됩니다."],
    still_running_continue => ["Start anyway", "起動処理を続ける", "继续启动", "繼續啟動", "계속 시작"],
    gate_unknown_title => ["Check the official site", "公式サイトを確認してください", "请查看官网", "請查看官網", "공식 사이트를 확인하세요"],
    gate_unknown_body => ["The game page is asking for something the Hub can't show. Open it on the official site.", "ゲームページに Hub で表示できない確認があります。公式サイトで開いてください。", "游戏页面需要确认 Hub 无法显示的内容，请在官网打开。", "遊戲頁面需要確認 Hub 無法顯示的內容，請在官網開啟。", "게임 페이지에 Hub에서 표시할 수 없는 확인 항목이 있습니다. 공식 사이트에서 여세요."],
    remove_title => ["Remove {} from the library?", "{} をライブラリから外しますか？", "要从库中移除 {} 吗？", "要從收藏庫移除 {} 嗎？", "{}을(를) 라이브러리에서 제거할까요?"],
    remove_body => ["Game files stay in {}.", "ゲームファイルは {} に残ります。", "游戏文件仍保留在 {}。", "遊戲檔案仍保留在 {}。", "게임 파일은 {}에 남아 있습니다."],
    downloads => ["Downloads", "ダウンロード", "下载", "下載", "다운로드"],
    no_downloads => ["No downloads", "ダウンロードはありません", "没有下载任务", "沒有下載項目", "다운로드 없음"],
    queue_position => ["Position {}", "{} 番目", "第 {} 位", "第 {} 位", "{}번째"],
    waiting_for_network => ["Waiting for network", "ネットワークを待機中", "正在等待网络", "正在等待網路", "네트워크 대기 중"],
    queued_note => ["Starts after the current download.", "現在のダウンロードの後に開始します。", "将在当前下载完成后开始。", "將在目前下載完成後開始。", "현재 다운로드가 끝난 후 시작합니다."],
    move_to_front => ["Move to front", "先頭に移動", "移到最前", "移至最前", "맨 앞으로 이동"],
    move_up => ["Move up", "上へ移動", "上移", "上移", "위로 이동"],
    remove_from_queue => ["Remove from queue", "キューから削除", "从队列移除", "從佇列移除", "대기열에서 제거"],
    install_elsewhere => ["Install elsewhere", "別の場所にインストール", "安装到其他位置", "安裝到其他位置", "다른 폴더에 설치"],
    discard_install => ["Cancel install", "インストールを中止", "取消安装", "取消安裝", "설치 취소"],
    discard_install_hint => ["Cancel this attempt without deleting files", "ファイルを残してインストールを中止", "取消本次安装，但不删除文件", "取消本次安裝，但不刪除檔案", "파일을 삭제하지 않고 설치 취소"],
    found_at => ["Found at {}", "{} で見つかりました", "在 {} 发现", "在 {} 找到", "{}에서 찾음"],
    add => ["Add", "追加", "添加", "新增", "추가"],
    settings => ["Settings", "設定", "设置", "設定", "설정"],
    general => ["General", "一般", "常规", "一般", "일반"],
    account => ["Account", "アカウント", "账户", "帳戶", "계정"],
    about => ["About", "このアプリについて", "关于", "關於", "정보"],
    language => ["Language", "言語", "语言", "語言", "언어"],
    close_button => ["Close button", "閉じるボタン", "关闭按钮", "關閉按鈕", "닫기 버튼"],
    close_tray => ["Minimize to tray", "トレイに最小化", "最小化到托盘", "最小化到系統匣", "트레이로 최소화"],
    close_exit => ["Exit", "終了", "退出", "結束", "종료"],
    default_folder => ["Default install folder", "既定のインストール先", "默认安装位置", "預設安裝位置", "기본 설치 폴더"],
    parallel => ["Parallel downloads", "同時ダウンロード数", "并行下载数", "同時下載數", "동시 다운로드"],
    remember => ["Stay signed in", "ログイン状態を保持", "保持登录", "保持登入", "로그인 유지"],
    unofficial => ["Unofficial. Not affiliated with or endorsed by KONAMI.", "非公式アプリです。KONAMI とは関係ありません。", "非官方应用，与 KONAMI 无关。", "非官方應用程式，與 KONAMI 無關。", "비공식 앱이며 KONAMI와 관련이 없습니다."],
    license => ["MIT License", "MIT ライセンス", "MIT 许可证", "MIT 授權", "MIT 라이선스"],
    data_folder => ["Open data folder", "データフォルダーを開く", "打开数据文件夹", "開啟資料夾", "데이터 폴더 열기"],
    sign_in => ["Sign in", "ログイン", "登录", "登入", "로그인"],
    signing_in => ["Signing in", "ログイン中", "正在登录", "正在登入", "로그인 중"],
    sign_out => ["Sign out", "ログアウト", "退出登录", "登出", "로그아웃"],
    sign_in_window => ["Sign in with KONAMI ID", "KONAMI ID でログイン", "使用 KONAMI ID 登录", "使用 KONAMI ID 登入", "KONAMI ID로 로그인"],
    login_id => ["KONAMI ID or email address", "KONAMI ID またはメールアドレス", "KONAMI ID 或电子邮件地址", "KONAMI ID 或電子郵件地址", "KONAMI ID 또는 이메일 주소"],
    password => ["Password", "パスワード", "密码", "密碼", "비밀번호"],
    next => ["Next", "次へ", "下一步", "下一步", "다음"],
    passkey => ["Sign in with a passkey", "パスキーでログイン", "使用通行密钥登录", "使用密碼金鑰登入", "패스키로 로그인"],
    passkey_wait => ["Continue in the Windows Security window.", "Windows セキュリティの画面で続けてください。", "请在 Windows 安全中心窗口中继续。", "請在 Windows 安全性視窗中繼續。", "Windows 보안 창에서 계속하세요."],
    captcha => ["Select every picture of this character.", "このキャラクターをすべて選んでください。", "请选择所有该角色的图片。", "請選擇所有該角色的圖片。", "이 캐릭터의 그림을 모두 고르세요."],
    new_pictures => ["New pictures", "画像を変更", "换一组图片", "換一組圖片", "다른 그림"],
    forgot_password => ["Forgot password", "パスワードを忘れた方", "忘记密码", "忘記密碼", "비밀번호 찾기"],
    code_email => ["Enter the 6-digit code sent to your email address.", "メールで届いた6桁の確認コードを入力してください。", "请输入发送到电子邮箱的 6 位验证码。", "請輸入寄到電子郵件的 6 位數驗證碼。", "이메일로 받은 6자리 확인 코드를 입력하세요."],
    code_sms => ["Enter the 6-digit code sent to your phone.", "SMS で届いた6桁の確認コードを入力してください。", "请输入短信收到的 6 位验证码。", "請輸入簡訊收到的 6 位數驗證碼。", "문자로 받은 6자리 확인 코드를 입력하세요."],
    trust_device => ["Don't ask on this device again", "この端末では次回から省略", "此设备下次不再询问", "此裝置下次不再詢問", "이 기기에서 다시 묻지 않기"],
    resend_code => ["Send a new code", "確認コードを再送信", "重新发送验证码", "重新傳送驗證碼", "코드 다시 보내기"],
    website_step => ["My KONAMI needs one more step. Finish it on the website, then sign in again.", "My KONAMI で追加の手続きが必要です。サイトで手続き後、もう一度ログインしてください。", "My KONAMI 需要额外步骤。请在网站上完成后重新登录。", "My KONAMI 需要額外步驟。請在網站上完成後重新登入。", "My KONAMI에서 추가 절차가 필요합니다. 사이트에서 완료한 뒤 다시 로그인하세요."],
    e_credentials => ["Check your ID and password.", "ログイン ID とパスワードを確認してください。", "请检查账号和密码。", "請檢查帳號和密碼。", "ID와 비밀번호를 확인하세요."],
    e_captcha => ["The pictures were not selected correctly.", "画像の選択が正しくありません。", "图片选择不正确。", "圖片選擇不正確。", "그림 선택이 올바르지 않습니다."],
    e_code => ["The code is not correct.", "確認コードが正しくありません。", "验证码不正确。", "驗證碼不正確。", "확인 코드가 올바르지 않습니다."],
    e_passkey => ["Could not sign in with a passkey.", "パスキーでログインできませんでした。", "无法使用通行密钥登录。", "無法使用密碼金鑰登入。", "패스키로 로그인할 수 없습니다."],
    e_refused => ["KONAMI declined the sign-in. Try again in a moment.", "KONAMI にログインを拒否されました。しばらくしてからお試しください。", "KONAMI 拒绝了登录，请稍后再试。", "KONAMI 拒絕了登入，請稍後再試。", "KONAMI가 로그인을 거부했습니다. 잠시 후 다시 시도하세요."],
    e_signInUnavailable => ["Could not reach KONAMI ID. Check your connection.", "KONAMI ID に接続できませんでした。接続を確認してください。", "无法连接 KONAMI ID，请检查网络。", "無法連線 KONAMI ID，請檢查網路。", "KONAMI ID에 연결할 수 없습니다. 연결을 확인하세요."],
    eapass => ["e-amusement pass", "e-amusement pass", "e-amusement pass", "e-amusement pass", "e-amusement pass"],
    show_card => ["Show card number", "カード番号を表示", "显示卡号", "顯示卡號", "카드 번호 표시"],
    hide_card => ["Hide card number", "カード番号を隠す", "隐藏卡号", "隱藏卡號", "카드 번호 숨기기"],
    no_pass => ["No active pass", "有効なカードがありません", "没有可用的卡", "沒有可用的卡", "사용 중인 카드 없음"],
    paseli => ["PASELI", "PASELI", "PASELI", "PASELI", "PASELI"],
    balance => ["Balance", "残高", "余额", "餘額", "잔액"],
    points => ["Points", "ポイント", "积分", "點數", "포인트"],
    charge => ["Charge", "チャージ", "充值", "儲值", "충전"],
    pass_menu => ["e-amusement pass menu", "e-amusement passメニュー", "e-amusement pass 菜单", "e-amusement pass 選單", "e-amusement pass 메뉴"],
    coop => ["Shop", "購買部", "购买部", "購買部", "구매부"],
    course => ["Course subscription", "コース加入", "加入课程", "加入課程", "코스 가입"],
    mypage => ["My Page", "マイページ", "我的页面", "我的頁面", "마이페이지"],
    my_konami => ["My KONAMI", "My KONAMI", "My KONAMI", "My KONAMI", "My KONAMI"],
    update_found => ["{} has an update.", "{} の更新があります。", "{} 有可用更新。", "{} 有可用更新。", "{} 업데이트가 있습니다."],
    installed => ["{} is ready.", "{} の準備ができました。", "{} 已就绪。", "{} 已就緒。", "{} 준비 완료."],
    game_exited => ["{} closed with an error ({}).", "{} がエラーで終了しました（{}）。", "{} 异常退出（{}）。", "{} 異常結束（{}）。", "{}이(가) 오류로 종료되었습니다({})."],
    signed_out => ["Your sign-in expired.", "ログインの有効期限が切れました。", "登录已过期。", "登入已過期。", "로그인이 만료되었습니다."],
    show => ["Show Konaste Hub", "Konaste Hub を表示", "显示 Konaste Hub", "顯示 Konaste Hub", "Konaste Hub 표시"],
    quit => ["Quit", "終了", "退出", "結束", "종료"],
    left => ["{} left", "残り {}", "剩余 {}", "剩餘 {}", "{} 남음"],
    volforce => ["VOLFORCE", "VOLFORCE", "VOLFORCE", "VOLFORCE", "VOLFORCE"],
    skill_level => ["Skill", "スキル", "技能", "技能", "스킬"],
    play_count => ["Plays", "プレー回数", "游玩次数", "遊玩次數", "플레이 횟수"],
    last_play => ["Last play", "最終プレー", "上次游玩", "上次遊玩", "마지막 플레이"],

    // Errors from the native core. Keys match HubError codes.
    e_metadataCheckFailed => ["Could not check for updates.", "更新を確認できませんでした。", "无法检查更新。", "無法檢查更新。", "업데이트를 확인할 수 없습니다."],
    e_metadataInvalid => ["Update information could not be verified.", "更新情報を検証できませんでした。", "无法验证更新信息。", "無法驗證更新資訊。", "업데이트 정보를 확인할 수 없습니다."],
    e_componentsFailed => ["Could not download official components.", "公式コンポーネントをダウンロードできませんでした。", "无法下载官方组件。", "無法下載官方元件。", "공식 구성 요소를 받을 수 없습니다."],
    e_componentsMissing => ["Official components are missing.", "公式コンポーネントがありません。", "缺少官方组件。", "缺少官方元件。", "공식 구성 요소가 없습니다."],
    e_installationOutdated => ["Game files need updating.", "ゲームファイルの更新が必要です。", "游戏文件需要更新。", "遊戲檔案需要更新。", "게임 파일 업데이트가 필요합니다."],
    e_installationInvalid => ["Game files could not be verified. Verify files to repair.", "ゲームファイルを検証できませんでした。ファイルを検証して修復してください。", "无法校验游戏文件，请校验文件进行修复。", "無法驗證遊戲檔案，請驗證檔案以修復。", "게임 파일을 확인할 수 없습니다. 파일 확인으로 복구하세요."],
    e_notAnInstallation => ["This folder is not a game installation.", "このフォルダーはゲームのインストール先ではありません。", "此文件夹不是游戏安装位置。", "此資料夾不是遊戲安裝位置。", "이 폴더는 게임 설치 위치가 아닙니다."],
    e_destinationNotEmpty => ["Choose an empty folder.", "空のフォルダーを選んでください。", "请选择空文件夹。", "請選擇空資料夾。", "빈 폴더를 선택하세요."],
    e_destinationPathUnsupported => ["Choose a shorter path on a local drive using only ASCII characters.", "ローカルドライブ上の、ASCII 文字だけを使った短いパスを選んでください。", "请选择本地驱动器上仅含 ASCII 字符的较短路径。", "請選擇本機磁碟上僅含 ASCII 字元的較短路徑。", "로컬 드라이브에서 ASCII 문자만 사용한 짧은 경로를 선택하세요."],
    e_destinationRejected => ["Choose a folder outside existing installations.", "既存のインストール先の外を選んでください。", "请选择现有安装位置之外的文件夹。", "請選擇現有安裝位置以外的資料夾。", "기존 설치 위치 밖의 폴더를 선택하세요."],
    e_diskSpace => ["Not enough free space.", "空き容量が足りません。", "可用空间不足。", "可用空間不足。", "여유 공간이 부족합니다."],
    e_downloadFailed => ["Download stopped. Retry to continue.", "ダウンロードが停止しました。再試行で続行します。", "下载已停止，重试以继续。", "下載已停止，重試以繼續。", "다운로드가 중지되었습니다. 다시 시도하면 이어집니다."],
    e_updateFailed => ["The update failed. Retry to continue.", "更新に失敗しました。再試行で続行します。", "更新失败，重试以继续。", "更新失敗，重試以繼續。", "업데이트에 실패했습니다. 다시 시도하면 이어집니다."],
    e_updateRecoveryRequired => ["The update changed during installation. Install to another folder.", "インストール中に更新内容が変わりました。別のフォルダーにインストールしてください。", "安装期间更新内容已变化，请安装到其他文件夹。", "安裝期間更新內容已變更，請安裝到其他資料夾。", "설치 중 업데이트가 바뀌었습니다. 다른 폴더에 설치하세요."],
    e_updateRequired => ["A newer update is required. Retry.", "新しい更新が必要です。再試行してください。", "需要更新的版本，请重试。", "需要更新的版本，請重試。", "새 업데이트가 필요합니다. 다시 시도하세요."],
    e_gameRunning => ["Close the game first.", "先にゲームを終了してください。", "请先关闭游戏。", "請先關閉遊戲。", "먼저 게임을 종료하세요."],
    e_gameSettingsBusy => ["Close game settings first.", "先にゲーム設定を閉じてください。", "请先关闭游戏设置。", "請先關閉遊戲設定。", "먼저 게임 설정을 닫으세요."],
    e_gameSettingsUnavailable => ["Game settings could not be verified.", "ゲーム設定を検証できませんでした。", "无法验证游戏设置。", "無法驗證遊戲設定。", "게임 설정을 확인할 수 없습니다."],
    e_gameSettingsInvalid => ["This game's settings file could not be read. Open Game settings and save.", "このゲームの設定ファイルを読み込めませんでした。ゲーム設定を開いて保存してください。", "无法读取此游戏的设置文件。请打开游戏设置并保存。", "無法讀取此遊戲的設定檔。請開啟遊戲設定並儲存。", "이 게임의 설정 파일을 읽을 수 없습니다. 게임 설정을 열고 저장하세요."],
    e_gameSettingsFailed => ["Could not open game settings.", "ゲーム設定を開けませんでした。", "无法打开游戏设置。", "無法開啟遊戲設定。", "게임 설정을 열 수 없습니다."],
    e_elevationCancelled => ["Administrator permission is needed for this game's settings.", "このゲームの設定には管理者の許可が必要です。", "此游戏的设置需要管理员权限。", "此遊戲的設定需要系統管理員權限。", "이 게임의 설정에는 관리자 권한이 필요합니다."],
    e_gameStartFailed => ["The game could not start.", "ゲームを起動できませんでした。", "游戏无法启动。", "遊戲無法啟動。", "게임을 시작할 수 없습니다."],
    e_gameModeRequired => ["Choose GuitarFreaks or DrumMania.", "ギターフリークスかドラムマニアを選んでください。", "请选择 GuitarFreaks 或 DrumMania。", "請選擇 GuitarFreaks 或 DrumMania。", "GuitarFreaks 또는 DrumMania를 선택하세요."],
    e_gameModeBusy => ["Close GITADORA before changing instrument.", "楽器を変える前に GITADORA を終了してください。", "更换乐器前请关闭 GITADORA。", "更換樂器前請關閉 GITADORA。", "악기를 바꾸기 전에 GITADORA를 종료하세요."],
    e_launchDenied => ["The official service did not allow this launch. Check the game's page on the official site.", "公式サービスが起動を許可しませんでした。公式サイトのゲームページを確認してください。", "官方服务未允许启动，请在官网查看游戏页面。", "官方服務未允許啟動，請在官網查看遊戲頁面。", "공식 서비스가 실행을 허용하지 않았습니다. 공식 사이트의 게임 페이지를 확인하세요."],
    e_launchVerificationFailed => ["Could not verify launch requirements. Retry.", "起動条件を確認できませんでした。再試行してください。", "无法验证启动条件，请重试。", "無法驗證啟動條件，請重試。", "실행 조건을 확인할 수 없습니다. 다시 시도하세요."],
    e_launchBusy => ["A launch is already in progress.", "すでに起動処理中です。", "已有启动正在进行。", "已有啟動正在進行。", "이미 실행 중입니다."],
    e_launchUnavailable => ["Play is not available yet.", "まだプレイできません。", "暂时无法游玩。", "暫時無法遊玩。", "아직 플레이할 수 없습니다."],
    e_checkChanged => ["Game state changed. Try again.", "ゲームの状態が変わりました。もう一度お試しください。", "游戏状态已变化，请重试。", "遊戲狀態已變更，請重試。", "게임 상태가 바뀌었습니다. 다시 시도하세요."],
    e_maintenance => ["Online services are under maintenance.", "オンラインサービスはメンテナンス中です。", "在线服务维护中。", "線上服務維護中。", "온라인 서비스 점검 중입니다."],
    e_availabilityUnavailable => ["Could not reach the official service. Check your connection.", "公式サービスに接続できませんでした。接続を確認してください。", "无法连接官方服务，请检查网络。", "無法連線官方服務，請檢查網路。", "공식 서비스에 연결할 수 없습니다. 연결을 확인하세요."],
    e_signInRequired => ["Sign in to play.", "プレイするにはログインしてください。", "请登录后游玩。", "請登入後遊玩。", "플레이하려면 로그인하세요."],
    e_accountUnavailable => ["Could not read account details.", "アカウント情報を取得できませんでした。", "无法读取账户信息。", "無法讀取帳戶資訊。", "계정 정보를 읽을 수 없습니다."],
    e_loginUnavailable => ["Could not start sign-in.", "ログインを開始できませんでした。", "无法开始登录。", "無法開始登入。", "로그인을 시작할 수 없습니다."],
    e_processCheck => ["Could not check whether the game is running.", "ゲームの起動状態を確認できませんでした。", "无法确认游戏是否在运行。", "無法確認遊戲是否在執行。", "게임 실행 여부를 확인할 수 없습니다."],
    e_unsupportedTitle => ["This title is not supported yet.", "このタイトルはまだ対応していません。", "暂不支持此游戏。", "尚未支援此遊戲。", "아직 지원하지 않는 게임입니다."],
    e_updateBusy => ["An update is already running.", "すでに更新中です。", "已有更新正在进行。", "已有更新正在進行。", "이미 업데이트 중입니다."],
    hub_update => ["Update to {}", "{} に更新", "更新到 {}", "更新至 {}", "{}(으)로 업데이트"],
    hub_updating => ["Updating", "更新中", "正在更新", "正在更新", "업데이트 중"],
    e_hubUpdateBusy => ["Close running games and settings, and pause or finish downloads before updating Konaste Hub.", "Konaste Hub を更新する前に、ゲームと設定を閉じ、ダウンロードを一時停止するか完了してください。", "更新 Konaste Hub 前，请关闭正在运行的游戏和设置，并暂停或完成下载。", "更新 Konaste Hub 前，請關閉執行中的遊戲和設定，並暫停或完成下載。", "Konaste Hub를 업데이트하기 전에 실행 중인 게임과 설정을 닫고 다운로드를 일시 중지하거나 완료하세요."],
    e_hubUpdateDownload => ["Could not download the Konaste Hub update. Try again later.", "Konaste Hub の更新をダウンロードできませんでした。しばらくしてからもう一度お試しください。", "无法下载 Konaste Hub 更新。请稍后重试。", "無法下載 Konaste Hub 更新。請稍後再試。", "Konaste Hub 업데이트를 다운로드할 수 없습니다. 나중에 다시 시도하세요."],
    e_hubUpdateVerify => ["The Konaste Hub update did not match its published checksum and was not installed.", "Konaste Hub の更新が公開されたチェックサムと一致しなかったため、インストールしませんでした。", "Konaste Hub 更新与发布的校验值不符，未安装。", "Konaste Hub 更新與公布的檢查碼不符，未安裝。", "Konaste Hub 업데이트가 게시된 체크섬과 일치하지 않아 설치하지 않았습니다."],
    e_hubUpdateRun => ["Could not start the Konaste Hub update.", "Konaste Hub の更新を開始できませんでした。", "无法启动 Konaste Hub 更新。", "無法啟動 Konaste Hub 更新。", "Konaste Hub 업데이트를 시작할 수 없습니다."],
    e_quitBusy => ["Close running games and settings, and pause or finish downloads before quitting.", "終了する前にゲームと設定を閉じ、ダウンロードを一時停止するか完了を待ってください。", "退出前请关闭游戏和设置，并暂停或完成下载。", "結束前請關閉遊戲與設定，並暫停或完成下載。", "종료하기 전에 게임과 설정을 닫고 다운로드를 일시 중지하거나 완료하세요."],
    e_filesystem => ["Could not access the folder.", "フォルダーにアクセスできませんでした。", "无法访问该文件夹。", "無法存取該資料夾。", "폴더에 접근할 수 없습니다."],
    e_browserUnavailable => ["Could not open your browser.", "ブラウザーを開けませんでした。", "无法打开浏览器。", "無法開啟瀏覽器。", "브라우저를 열 수 없습니다."],
    e_unknown => ["Something went wrong.", "エラーが発生しました。", "出现错误。", "發生錯誤。", "오류가 발생했습니다."],
}

pub fn error(locale: Locale, code: &str) -> &'static str {
    let key = match code {
        "metadataCheckFailed" | "metadataUnavailable" => K::e_metadataCheckFailed,
        "metadataInvalid" => K::e_metadataInvalid,
        "componentsFailed" => K::e_componentsFailed,
        "componentsMissing" => K::e_componentsMissing,
        "installationOutdated" => K::e_installationOutdated,
        "installationInvalid" => K::e_installationInvalid,
        "gameSettingsInvalid" => K::e_gameSettingsInvalid,
        "notAnInstallation" => K::e_notAnInstallation,
        "destinationNotEmpty" => K::e_destinationNotEmpty,
        "destinationPathUnsupported" => K::e_destinationPathUnsupported,
        "destinationRejected" => K::e_destinationRejected,
        "diskSpace" => K::e_diskSpace,
        "downloadFailed" => K::e_downloadFailed,
        "updateFailed" | "updateChanged" => K::e_updateFailed,
        "updateRecoveryRequired" => K::e_updateRecoveryRequired,
        "updateRequired" => K::e_updateRequired,
        "gameRunning" => K::e_gameRunning,
        "gameSettingsBusy" => K::e_gameSettingsBusy,
        "gameSettingsUnavailable" => K::e_gameSettingsUnavailable,
        "gameSettingsFailed" => K::e_gameSettingsFailed,
        "elevationCancelled" => K::e_elevationCancelled,
        "gameStartFailed" => K::e_gameStartFailed,
        "gameModeRequired" => K::e_gameModeRequired,
        "gameModeBusy" => K::e_gameModeBusy,
        "launchDenied" => K::e_launchDenied,
        "launchVerificationFailed" => K::e_launchVerificationFailed,
        "launchBusy" => K::e_launchBusy,
        "launchUnavailable" | "launchUnsupported" => K::e_launchUnavailable,
        "checkChanged" => K::e_checkChanged,
        "maintenance" => K::e_maintenance,
        "availabilityUnavailable" => K::e_availabilityUnavailable,
        "signInRequired" => K::e_signInRequired,
        "accountUnavailable" => K::e_accountUnavailable,
        "loginUnavailable" | "loginCancelled" | "loginTimeout" => K::e_loginUnavailable,
        "processCheck" => K::e_processCheck,
        "unsupportedTitle" => K::e_unsupportedTitle,
        "updateBusy" => K::e_updateBusy,
        "quitBusy" => K::e_quitBusy,
        "hubUpdateBusy" => K::e_hubUpdateBusy,
        "hubUpdateDownload" => K::e_hubUpdateDownload,
        "hubUpdateVerify" => K::e_hubUpdateVerify,
        "hubUpdateRun" => K::e_hubUpdateRun,
        "filesystem" => K::e_filesystem,
        "browserUnavailable" => K::e_browserUnavailable,
        _ => K::e_unknown,
    };
    t(locale, key)
}

/// Replace `{}` placeholders in order.
pub fn fill(template: &str, values: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    let mut values = values.iter();
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        out.push_str(values.next().copied().unwrap_or(""));
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

pub fn bytes(value: u64) -> String {
    let gib = value as f64 / (1024.0 * 1024.0 * 1024.0);
    if gib >= 10.0 {
        format!("{gib:.1} GB")
    } else if gib >= 1.0 {
        format!("{gib:.2} GB")
    } else {
        format!("{:.0} MB", value as f64 / (1024.0 * 1024.0))
    }
}

pub fn duration(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}
