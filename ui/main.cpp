// =============================================================================
//  ui/main.cpp
//
//  AudioPlayerX86 主入口。Qt Quick QML 引擎启动。
// =============================================================================

#include "bridge/PlayerViewModel.h"
#include "bridge/CoverImageProvider.h"
#include "bridge/ShortcutsViewModel.h"
#include "platform/taskbar/TaskbarButtons.h"
#include "platform/taskbar/JumpList.h"

#include <QGuiApplication>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QAbstractNativeEventFilter>
#include <QIcon>
#include <QFile>
#include <QMutex>
#include <QMutexLocker>
#include <QDateTime>
#include <QDir>
#include <QStandardPaths>

#ifdef _WIN32
#  ifndef WIN32_LEAN_AND_MEAN
#    define WIN32_LEAN_AND_MEAN
#  endif
#  include <windows.h>
#  include <dwmapi.h>
// 旧版 Windows SDK 没有定义这两个,自己补上 (值取自 Win11 SDK)
#  ifndef DWMWA_WINDOW_CORNER_PREFERENCE
#    define DWMWA_WINDOW_CORNER_PREFERENCE 33
#  endif
#  ifndef DWMWCP_ROUND
#    define DWMWCP_ROUND 2
#  endif
#endif

namespace {

QFile g_logFile;
QMutex g_logMutex;

void apxMessageHandler(QtMsgType type, const QMessageLogContext& ctx, const QString& msg)
{
    QMutexLocker lock(&g_logMutex);
    if (!g_logFile.isOpen()) {
        QString dir = QCoreApplication::applicationDirPath();
        if (dir.isEmpty()) dir = QDir::currentPath();
        g_logFile.setFileName(dir + "/apx.log");
        g_logFile.open(QIODevice::WriteOnly | QIODevice::Truncate | QIODevice::Text);
    }
    const char* level = "I";
    switch (type) {
        case QtDebugMsg:    level = "D"; break;
        case QtInfoMsg:     level = "I"; break;
        case QtWarningMsg:  level = "W"; break;
        case QtCriticalMsg: level = "C"; break;
        case QtFatalMsg:    level = "F"; break;
    }
    QString line = QString("[%1] %2: %3")
        .arg(QDateTime::currentDateTime().toString("HH:mm:ss.zzz"))
        .arg(level)
        .arg(msg);
    if (ctx.file && *ctx.file) {
        line += QString(" (%1:%2)").arg(ctx.file).arg(ctx.line);
    }
    line += "\n";
    if (g_logFile.isOpen()) {
        g_logFile.write(line.toUtf8());
        g_logFile.flush();
    }
}

} // namespace

#ifdef _WIN32
// 截获 WM_COMMAND,把任务栏缩略图按钮事件路给 PlayerViewModel
class TaskbarEventFilter : public QAbstractNativeEventFilter {
public:
    explicit TaskbarEventFilter(apx::ui::PlayerViewModel* vm) : vm_(vm) {}

    bool nativeEventFilter(const QByteArray& eventType,
                           void* message,
                           qintptr* /*result*/) override
    {
        if (eventType != "windows_generic_MSG") return false;
        MSG* msg = static_cast<MSG*>(message);
        if (msg->message == WM_COMMAND) {
            UINT cmd = LOWORD(msg->wParam);
            if (auto* tb = vm_->taskbarButtons()) {
                if (tb->handleCommand(cmd)) return true;
            }
        }
        // explorer.exe 重启后会广播 WM_TaskbarButtonCreated，需要重建按钮。
        if (auto* tb = vm_->taskbarButtons()) {
            const uint32_t restartMsg = tb->taskbarCreatedMessageId();
            if (restartMsg != 0 && msg->message == restartMsg) {
                tb->onTaskbarRestart();
                return false;   // 让其它监听者也能收到
            }
        }
        return false;
    }

private:
    apx::ui::PlayerViewModel* vm_;
};
#endif


int main(int argc, char* argv[])
{
    qInstallMessageHandler(apxMessageHandler);

    QCoreApplication::setAttribute(Qt::AA_DontCreateNativeWidgetSiblings);

    QGuiApplication::setApplicationName("SeraphAudioPlayer");
    QGuiApplication::setOrganizationName("SeraphAudioPlayer");
    QGuiApplication::setApplicationVersion("0.3.3");

    // 强制使用 Basic 样式 — 让 Slider 等控件的自定义 background/handle 生效
    // (Windows 默认会用原生样式,不支持自定义)
    QQuickStyle::setStyle("Basic");

    // 启用高 DPI 支持
#if QT_VERSION < QT_VERSION_CHECK(6, 0, 0)
    QCoreApplication::setAttribute(Qt::AA_EnableHighDpiScaling);
#endif

    QGuiApplication app(argc, argv);
    app.setWindowIcon(QIcon(":/app_icon.svg"));

    qInfo() << "SeraphAudioPlayer startup";
    qInfo() << "app dir:" << QCoreApplication::applicationDirPath();

    // 一次性注册 Jump List 任务
    apx::JumpList::install();

    // 初始化 ViewModel
    apx::ui::PlayerViewModel playerVM;

#ifdef _WIN32
    TaskbarEventFilter tbFilter(&playerVM);
    app.installNativeEventFilter(&tbFilter);
#endif

    // 解析命令行参数
    const QStringList args = QGuiApplication::arguments();
    for (int i = 1; i < args.size(); ++i) {
        const QString& a = args[i];
        if (a == "--play")        { QMetaObject::invokeMethod(&playerVM, "play",     Qt::QueuedConnection); }
        else if (a == "--pause")  { QMetaObject::invokeMethod(&playerVM, "pause",    Qt::QueuedConnection); }
        else if (a == "--next")   { QMetaObject::invokeMethod(&playerVM, "next",     Qt::QueuedConnection); }
        else if (a == "--prev")   { QMetaObject::invokeMethod(&playerVM, "previous", Qt::QueuedConnection); }
        else if (a == "--open")   { /* 由 QML 端 FileDialog 处理 - 跳过 */ }
        else if (!a.startsWith("-")) {
            playerVM.openFile(a);
        }
    }

    QQmlApplicationEngine engine;

    // ShortcutsViewModel 必须在 engine 之前声明，确保栈析构顺序：
    // engine 先于 ViewModel 销毁，避免 QML binding 在 VM 已析构后再访问。
    apx::ui::ShortcutsViewModel shortcutsVM;

    // 注册封面 ImageProvider
    engine.addImageProvider("covers", new apx::ui::CoverImageProvider());

    // 注册到 QML 上下文
    engine.rootContext()->setContextProperty("playerVM", &playerVM);
    engine.rootContext()->setContextProperty("shortcutsVM", &shortcutsVM);

    // 捕获 QML 加载警告
    QObject::connect(&engine, &QQmlApplicationEngine::warnings,
                     [](const QList<QQmlError>& list) {
        for (const auto& e : list) qWarning().noquote() << "[QML]" << e.toString();
    });

    // 加载主 QML
    const QUrl url(QStringLiteral("qrc:/qml/main.qml"));
    QObject::connect(&engine, &QQmlApplicationEngine::objectCreated,
                     &app, [url, &playerVM](QObject *obj, const QUrl &objUrl) {
        if (!obj && url == objUrl) {
            qCritical() << "QML root object failed to create:" << objUrl;
            QCoreApplication::exit(-1);
            return;
        }
        if (url == objUrl) {
            // 把主窗口的 HWND 绑给 SMTC / 任务栏按钮
            if (auto* win = qobject_cast<QQuickWindow*>(obj)) {
                playerVM.attachWindow(reinterpret_cast<void*>(win->winId()));
#ifdef _WIN32
                // ---- Win11 系统原生圆角 + 阴影 (frameless / WS_POPUP 兼容) ----
                // 1) DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND
                //    让 DWM 在合成阶段把窗口外角做抗锯齿圆角剪裁。
                // 2) DwmExtendFrameIntoClientArea(MARGINS{1,1,1,1})
                //    告诉 DWM "把窗口框架延伸进 client area",DWM 会接管窗口
                //    外侧阴影的合成。frameless WS_POPUP 默认 DWM 不画阴影,
                //    此调用是补齐这一项的关键。1px 的延伸量肉眼不可见。
                // Win10 / 旧系统上两个 API 都返回失败码但不会崩,视觉退化为
                // 之前的"无圆角无阴影 frameless 窗口"行为。
                HWND hwnd = reinterpret_cast<HWND>(win->winId());
                UINT pref = DWMWCP_ROUND;
                ::DwmSetWindowAttribute(hwnd,
                                        DWMWA_WINDOW_CORNER_PREFERENCE,
                                        &pref, sizeof(pref));
                MARGINS shadow_margins{1, 1, 1, 1};
                ::DwmExtendFrameIntoClientArea(hwnd, &shadow_margins);
#endif
            }
        }
    }, Qt::QueuedConnection);
    engine.load(url);

    return app.exec();
}
