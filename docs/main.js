const translations = {
  en: {
    "nav.features": "Features",
    "nav.useCases": "Use cases",
    "nav.howItWorks": "How it works",
    "nav.faq": "FAQ",
    "nav.github": "GitHub",
    "nav.repoCta": "View repo",
    "a11y.skipLink": "Skip to content",
    "a11y.brandHome": "CarryTalk home",
    "a11y.primaryNav": "Primary navigation",
    "a11y.languageSwitcher": "Language switcher",
    "a11y.heroHighlights": "Key product highlights",
    "a11y.logoAlt": "CarryTalk logo",
    "a11y.screenshot1Alt": "CarryTalk desktop interface showing a live transcript session",
    "a11y.screenshot2Alt": "CarryTalk settings and controls for audio and translation configuration",
    "hero.eyebrow": "Portable desktop app for live language workflows",
    "hero.title": "Real-time transcription and translation, built for desktop.",
    "hero.lead": "CarryTalk helps you capture speech from your microphone, system audio, or both, then follow live transcription and translation in one focused desktop interface.",
    "hero.primaryCta": "Download from Releases",
    "hero.secondaryCta": "Build from source",
    "hero.point1": "Real-time transcript and translation updates",
    "hero.point2": "Timestamped session history stored locally",
    "hero.point3": "English and Vietnamese interface support",
    "problem.eyebrow": "Problem → Solution",
    "problem.title": "From scattered live audio to a portable workflow you can reopen.",
    "problem.problemTitle": "The problem",
    "problem.problemBody": "Live conversations, calls, and videos move quickly. It is easy to miss context, lose translated meaning, or rely on tools that do not keep a usable local record.",
    "problem.solutionTitle": "The CarryTalk approach",
    "problem.solutionBody": "CarryTalk combines real-time transcription, translation, flexible audio capture, and local session persistence so you can follow what is happening now and recover the session later when needed.",
    "features.eyebrow": "Features",
    "features.title": "Six practical capabilities confirmed in the product.",
    "features.card1.title": "Real-time transcript updates",
    "features.card1.body": "Follow incoming speech as transcript segments update live during an active session.",
    "features.card2.title": "Live translation view",
    "features.card2.body": "Enable translation and track translated text alongside the original transcript flow.",
    "features.card3.title": "Session control states",
    "features.card3.body": "Start, pause, resume, and stop sessions without leaving the main recording workflow.",
    "features.card4.title": "Flexible audio capture",
    "features.card4.body": "Capture from microphone, system audio, or mixed input depending on your setup.",
    "features.card5.title": "Timestamped transcript records",
    "features.card5.body": "Transcript segments include timing data so sessions remain easier to review and trace.",
    "features.card6.title": "Local persistence and recovery",
    "features.card6.body": "Session data is stored locally and interrupted sessions can be recovered on the next app startup.",
    "useCases.eyebrow": "Use cases",
    "useCases.title": "Built for everyday listening and translation scenarios.",
    "useCases.case1.title": "Meetings and interviews",
    "useCases.case1.body": "Keep up with spoken details and revisit a locally saved transcript after the session.",
    "useCases.case2.title": "Online calls and streams",
    "useCases.case2.body": "Use system or mixed capture when audio comes from your computer output.",
    "useCases.case3.title": "Bilingual note-taking",
    "useCases.case3.body": "Watch original and translated text together while the conversation is still happening.",
    "useCases.case4.title": "Personal review workflows",
    "useCases.case4.body": "Return to timestamped transcript records without depending on a browser tab staying open.",
    "useCases.case5.title": "Portable desktop setups",
    "useCases.case5.body": "Run a desktop-first workflow that keeps app settings and session data on the local machine.",
    "how.eyebrow": "How it works",
    "how.title": "Four steps from input to saved session.",
    "how.step1.title": "Choose your capture mode",
    "how.step1.body": "Select microphone, system audio, or mixed capture, then pick the available devices.",
    "how.step2.title": "Start a live session",
    "how.step2.body": "Begin recording and watch the app move through its active session states.",
    "how.step3.title": "Follow transcript and translation",
    "how.step3.body": "Read live transcript updates with translated text when translation is enabled.",
    "how.step4.title": "Pause, resume, stop, and reopen later",
    "how.step4.body": "Keep control over the session lifecycle and rely on local persistence and recovery support.",
    "why.eyebrow": "Why CarryTalk",
    "why.title": "Focused on practical desktop transcription instead of noisy promises.",
    "why.item1.title": "Portable by design",
    "why.item1.body": "The app is built as a desktop application and stores runtime data locally.",
    "why.item2.title": "Live workflow first",
    "why.item2.body": "It is designed around real-time listening, session state handling, and active transcript updates.",
    "why.item3.title": "Recovery-aware",
    "why.item3.body": "Interrupted sessions are not treated as disposable, which makes the workflow more resilient.",
    "why.item4.title": "Simple language access",
    "why.item4.body": "The interface already supports English and Vietnamese, matching the product’s current language setup.",
    "oss.eyebrow": "Open-source / GitHub",
    "oss.title": "Explore the code, download releases, or build it yourself.",
    "oss.body": "CarryTalk is available on GitHub. The safest calls to action today are browsing the repository, downloading published releases, or building from source in your own environment.",
    "oss.cta1": "Open GitHub repository",
    "oss.cta2": "Browse GitHub Releases",
    "faq.eyebrow": "FAQ",
    "faq.title": "Common questions, answered carefully.",
    "faq.q1.q": "What is CarryTalk?",
    "faq.q1.a": "CarryTalk is a Tauri desktop app for portable real-time transcription and translation.",
    "faq.q2.q": "Can it capture more than microphone input?",
    "faq.q2.a": "Yes. The app includes microphone, system audio, and mixed capture modes, depending on runtime support in the environment.",
    "faq.q3.q": "Does it support session controls?",
    "faq.q3.a": "Yes. The current app flow includes start, pause, resume, and stop controls for sessions.",
    "faq.q4.q": "Are transcripts saved locally?",
    "faq.q4.a": "Yes. Session data is persisted locally, and the app includes startup recovery for interrupted sessions.",
    "faq.q5.q": "Does the interface support multiple languages?",
    "faq.q5.a": "Yes. The application already includes English and Vietnamese UI support.",
    "faq.q6.q": "How should I try it today?",
    "faq.q6.a": "Use the GitHub repository, GitHub Releases, or build from source. Those are the safest public entry points confirmed for the project right now.",
    "final.eyebrow": "Get started",
    "final.title": "Start with the repo. Download a release when available. Build when you need control.",
    "final.body": "CarryTalk is best explored as an open desktop project: inspect the code, review the release page, and run it locally when it fits your workflow.",
    "final.cta1": "Visit GitHub",
    "final.cta2": "Open Releases",
    "footer.copy": "CarryTalk is a portable desktop app for real-time transcription and translation.",
    "footer.rights": "Copyright"
  },
  vi: {
    "nav.features": "Tính năng",
    "nav.useCases": "Tình huống dùng",
    "nav.howItWorks": "Cách hoạt động",
    "nav.faq": "FAQ",
    "nav.github": "GitHub",
    "nav.repoCta": "Xem repo",
    "a11y.skipLink": "Bỏ qua để tới nội dung chính",
    "a11y.brandHome": "Trang chủ CarryTalk",
    "a11y.primaryNav": "Điều hướng chính",
    "a11y.languageSwitcher": "Bộ chuyển ngôn ngữ",
    "a11y.heroHighlights": "Các điểm nổi bật chính của sản phẩm",
    "a11y.logoAlt": "Logo CarryTalk",
    "a11y.screenshot1Alt": "Giao diện desktop CarryTalk đang hiển thị một phiên transcript trực tiếp",
    "a11y.screenshot2Alt": "Phần cài đặt và điều khiển CarryTalk cho cấu hình âm thanh và dịch",
    "hero.eyebrow": "Ứng dụng desktop portable cho workflow ngôn ngữ thời gian thực",
    "hero.title": "Phiên âm và dịch thời gian thực, được xây dựng cho desktop.",
    "hero.lead": "CarryTalk giúp bạn thu tiếng từ microphone, âm thanh hệ thống hoặc cả hai, rồi theo dõi phiên âm và bản dịch theo thời gian thực trong một giao diện desktop tập trung.",
    "hero.primaryCta": "Tải từ Releases",
    "hero.secondaryCta": "Tự build từ source",
    "hero.point1": "Cập nhật phiên âm và bản dịch theo thời gian thực",
    "hero.point2": "Lịch sử phiên có timestamp được lưu cục bộ",
    "hero.point3": "Hỗ trợ giao diện tiếng Anh và tiếng Việt",
    "problem.eyebrow": "Vấn đề → Giải pháp",
    "problem.title": "Từ luồng audio trực tiếp rời rạc đến workflow portable có thể mở lại.",
    "problem.problemTitle": "Vấn đề",
    "problem.problemBody": "Các cuộc trò chuyện trực tiếp, cuộc gọi và video diễn ra rất nhanh. Bạn dễ bỏ lỡ ngữ cảnh, mất ý nghĩa bản dịch hoặc phụ thuộc vào công cụ không giữ được bản ghi cục bộ đủ hữu ích.",
    "problem.solutionTitle": "Cách CarryTalk xử lý",
    "problem.solutionBody": "CarryTalk kết hợp phiên âm thời gian thực, dịch, thu âm linh hoạt và lưu phiên cục bộ để bạn vừa theo dõi diễn biến hiện tại vừa có thể khôi phục lại phiên sau đó khi cần.",
    "features.eyebrow": "Tính năng",
    "features.title": "Sáu khả năng thực tế đã được xác nhận trong sản phẩm.",
    "features.card1.title": "Cập nhật transcript thời gian thực",
    "features.card1.body": "Theo dõi lời nói đi vào khi các đoạn transcript được cập nhật trực tiếp trong phiên đang chạy.",
    "features.card2.title": "Hiển thị bản dịch trực tiếp",
    "features.card2.body": "Bật dịch và theo dõi văn bản đã dịch song song với luồng transcript gốc.",
    "features.card3.title": "Các trạng thái điều khiển phiên",
    "features.card3.body": "Bắt đầu, tạm dừng, tiếp tục và kết thúc phiên ngay trong workflow ghi chính.",
    "features.card4.title": "Thu âm linh hoạt",
    "features.card4.body": "Thu từ microphone, âm thanh hệ thống hoặc nguồn trộn tùy theo cấu hình của bạn.",
    "features.card5.title": "Bản ghi transcript có timestamp",
    "features.card5.body": "Các đoạn transcript có dữ liệu thời gian để việc xem lại và lần vết phiên trở nên dễ hơn.",
    "features.card6.title": "Lưu cục bộ và khôi phục",
    "features.card6.body": "Dữ liệu phiên được lưu cục bộ và các phiên bị gián đoạn có thể được khôi phục ở lần mở app tiếp theo.",
    "useCases.eyebrow": "Tình huống dùng",
    "useCases.title": "Phù hợp với các tình huống nghe và dịch thường ngày.",
    "useCases.case1.title": "Họp và phỏng vấn",
    "useCases.case1.body": "Bám theo chi tiết lời nói và xem lại transcript đã lưu cục bộ sau phiên.",
    "useCases.case2.title": "Cuộc gọi và livestream",
    "useCases.case2.body": "Dùng thu system hoặc mixed khi âm thanh phát ra từ máy tính của bạn.",
    "useCases.case3.title": "Ghi chú song ngữ",
    "useCases.case3.body": "Xem đồng thời văn bản gốc và bản dịch trong khi cuộc trò chuyện vẫn đang diễn ra.",
    "useCases.case4.title": "Workflow xem lại cá nhân",
    "useCases.case4.body": "Quay lại transcript có timestamp mà không cần phụ thuộc vào việc tab trình duyệt còn mở.",
    "useCases.case5.title": "Thiết lập desktop portable",
    "useCases.case5.body": "Chạy workflow desktop-first với settings và dữ liệu phiên được giữ trên máy cục bộ.",
    "how.eyebrow": "Cách hoạt động",
    "how.title": "Bốn bước từ đầu vào đến phiên đã lưu.",
    "how.step1.title": "Chọn chế độ thu âm",
    "how.step1.body": "Chọn microphone, âm thanh hệ thống hoặc mixed, rồi chọn thiết bị khả dụng.",
    "how.step2.title": "Bắt đầu phiên trực tiếp",
    "how.step2.body": "Bắt đầu ghi và theo dõi app chuyển qua các trạng thái phiên đang hoạt động.",
    "how.step3.title": "Theo dõi transcript và bản dịch",
    "how.step3.body": "Đọc transcript cập nhật trực tiếp cùng văn bản đã dịch khi bật tính năng dịch.",
    "how.step4.title": "Tạm dừng, tiếp tục, dừng và mở lại sau",
    "how.step4.body": "Giữ quyền kiểm soát vòng đời phiên và dựa vào khả năng lưu cục bộ cùng khôi phục.",
    "why.eyebrow": "Vì sao là CarryTalk",
    "why.title": "Tập trung vào workflow phiên âm desktop thực dụng thay vì những lời hứa lớn.",
    "why.item1.title": "Portable ngay từ thiết kế",
    "why.item1.body": "Ứng dụng được xây dựng cho desktop và lưu dữ liệu runtime trên máy cục bộ.",
    "why.item2.title": "Ưu tiên workflow trực tiếp",
    "why.item2.body": "Thiết kế xoay quanh việc nghe theo thời gian thực, xử lý trạng thái phiên và cập nhật transcript đang chạy.",
    "why.item3.title": "Có tính đến khôi phục",
    "why.item3.body": "Các phiên bị gián đoạn không bị coi là bỏ đi, giúp workflow bền hơn trong thực tế.",
    "why.item4.title": "Tiếp cận ngôn ngữ đơn giản",
    "why.item4.body": "Giao diện đã hỗ trợ tiếng Anh và tiếng Việt, phù hợp với thiết lập ngôn ngữ hiện tại của sản phẩm.",
    "oss.eyebrow": "Open-source / GitHub",
    "oss.title": "Xem code, tải releases hoặc tự build theo nhu cầu.",
    "oss.body": "CarryTalk có trên GitHub. Các CTA an toàn nhất hiện tại là xem repository, tải các bản phát hành đã publish hoặc tự build từ source trong môi trường của bạn.",
    "oss.cta1": "Mở GitHub repository",
    "oss.cta2": "Xem GitHub Releases",
    "faq.eyebrow": "FAQ",
    "faq.title": "Một số câu hỏi phổ biến, trả lời cẩn trọng.",
    "faq.q1.q": "CarryTalk là gì?",
    "faq.q1.a": "CarryTalk là ứng dụng desktop Tauri cho phiên âm và dịch thời gian thực theo hướng portable.",
    "faq.q2.q": "Có thu được nhiều hơn microphone không?",
    "faq.q2.a": "Có. App có các chế độ microphone, system audio và mixed, tùy theo khả năng runtime của môi trường.",
    "faq.q3.q": "Có hỗ trợ điều khiển phiên không?",
    "faq.q3.a": "Có. Flow hiện tại của app có điều khiển bắt đầu, tạm dừng, tiếp tục và dừng phiên.",
    "faq.q4.q": "Transcript có được lưu cục bộ không?",
    "faq.q4.a": "Có. Dữ liệu phiên được lưu cục bộ và app có cơ chế khôi phục khi khởi động lại nếu phiên trước bị gián đoạn.",
    "faq.q5.q": "Giao diện có hỗ trợ nhiều ngôn ngữ không?",
    "faq.q5.a": "Có. Ứng dụng hiện đã hỗ trợ giao diện tiếng Anh và tiếng Việt.",
    "faq.q6.q": "Nên thử sản phẩm bằng cách nào hôm nay?",
    "faq.q6.a": "Hãy dùng GitHub repository, GitHub Releases hoặc tự build từ source. Đây là các điểm vào công khai an toàn nhất đang được xác nhận cho dự án.",
    "final.eyebrow": "Bắt đầu",
    "final.title": "Bắt đầu từ repo. Tải release khi có. Tự build khi bạn cần quyền kiểm soát.",
    "final.body": "CarryTalk phù hợp để khám phá như một dự án desktop mở: đọc code, xem trang release và chạy cục bộ khi phù hợp với workflow của bạn.",
    "final.cta1": "Truy cập GitHub",
    "final.cta2": "Mở Releases",
    "footer.copy": "CarryTalk là ứng dụng desktop portable cho phiên âm và dịch thời gian thực.",
    "footer.rights": "Bản quyền"
  }
};

const STORAGE_KEY = "carrytalk.docs.lang";

function applyLanguage(lang) {
  const selected = translations[lang] ? lang : "en";
  const dict = translations[selected];

  document.documentElement.lang = selected;

  document.querySelectorAll("[data-i18n]").forEach((node) => {
    const key = node.getAttribute("data-i18n");
    if (!key || !(key in dict)) {
      return;
    }
    node.textContent = dict[key];
  });

  document.querySelectorAll("[data-i18n-aria-label]").forEach((node) => {
    const key = node.getAttribute("data-i18n-aria-label");
    if (!key || !(key in dict)) {
      return;
    }
    node.setAttribute("aria-label", dict[key]);
  });

  document.querySelectorAll("[data-i18n-alt]").forEach((node) => {
    const key = node.getAttribute("data-i18n-alt");
    if (!key || !(key in dict)) {
      return;
    }
    node.setAttribute("alt", dict[key]);
  });

  document.querySelectorAll(".lang-button").forEach((button) => {
    const isActive = button.getAttribute("data-lang") === selected;
    button.classList.toggle("is-active", isActive);
    button.setAttribute("aria-pressed", String(isActive));
  });

  try {
    localStorage.setItem(STORAGE_KEY, selected);
  } catch (_error) {
    // Ignore storage failures.
  }
}

function getInitialLanguage() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && translations[saved]) {
      return saved;
    }
  } catch (_error) {
    // Ignore storage failures.
  }
  return "en";
}

function setCurrentYear() {
  const yearNode = document.getElementById("current-year");
  if (yearNode) {
    yearNode.textContent = String(new Date().getFullYear());
  }
}

document.addEventListener("DOMContentLoaded", () => {
  setCurrentYear();
  applyLanguage(getInitialLanguage());

  document.querySelectorAll(".lang-button").forEach((button) => {
    button.addEventListener("click", () => {
      const lang = button.getAttribute("data-lang") || "en";
      applyLanguage(lang);
    });
  });
});
