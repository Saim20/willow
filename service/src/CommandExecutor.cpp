#include "CommandExecutor.hpp"
#include <cstdlib>
#include <iostream>
#include <fstream>
#include <ctime>
#include <iomanip>
#include <sstream>
#include <algorithm>
#include <vector>
#include <spawn.h>
#include <unistd.h>
#include <sys/wait.h>
#include <cctype>

extern char **environ;

namespace VoiceAssistant {

namespace {

int levenshteinDistance(const std::string& a, const std::string& b) {
    const size_t m = a.size();
    const size_t n = b.size();
    std::vector<std::vector<int>> dp(m + 1, std::vector<int>(n + 1));

    for (size_t i = 0; i <= m; ++i) dp[i][0] = static_cast<int>(i);
    for (size_t j = 0; j <= n; ++j) dp[0][j] = static_cast<int>(j);

    for (size_t i = 1; i <= m; ++i) {
        for (size_t j = 1; j <= n; ++j) {
            const int cost = (a[i - 1] == b[j - 1]) ? 0 : 1;
            dp[i][j] = std::min({
                dp[i - 1][j] + 1,
                dp[i][j - 1] + 1,
                dp[i - 1][j - 1] + cost
            });
        }
    }

    return dp[m][n];
}

double tokenOverlapScore(const std::string& text, const std::string& phrase) {
    auto tokenize = [](const std::string& input) {
        std::vector<std::string> tokens;
        std::string current;
        for (char c : input) {
            if (std::isalnum(static_cast<unsigned char>(c))) {
                current += c;
            } else if (!current.empty()) {
                tokens.push_back(current);
                current.clear();
            }
        }
        if (!current.empty()) {
            tokens.push_back(current);
        }
        return tokens;
    };

    const auto textTokens = tokenize(text);
    const auto phraseTokens = tokenize(phrase);
    if (phraseTokens.empty()) {
        return 0.0;
    }

    size_t matches = 0;
    for (const auto& phraseToken : phraseTokens) {
        for (const auto& textToken : textTokens) {
            if (textToken == phraseToken) {
                ++matches;
                break;
            }
        }
    }

    return static_cast<double>(matches) / phraseTokens.size();
}

int spawnProcess(const std::vector<std::string>& args) {
    if (args.empty()) {
        return -1;
    }

    std::vector<char*> argv;
    argv.reserve(args.size() + 1);
    for (const auto& arg : args) {
        argv.push_back(const_cast<char*>(arg.c_str()));
    }
    argv.push_back(nullptr);

    pid_t pid = 0;
    const int result = posix_spawnp(&pid, argv[0], nullptr, nullptr, argv.data(), environ);
    if (result != 0) {
        return result;
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        return -1;
    }

    if (WIFEXITED(status)) {
        return WEXITSTATUS(status);
    }

    return -1;
}

int spawnShellCommand(const std::string& command) {
    return spawnProcess({"/bin/sh", "-c", command});
}

std::vector<std::string> splitCommand(const std::string& command) {
    std::vector<std::string> parts;
    std::istringstream stream(command);
    std::string token;
    while (stream >> token) {
        parts.push_back(token);
    }
    return parts;
}

} // namespace

CommandExecutor::CommandExecutor()
    : m_logFile("/tmp/willow.log")
    , m_ydotoolAvailable(isYdotoolAvailable())
{
    applyDefaultContext();
    const char* home = std::getenv("HOME");
    if (home) {
        std::string contextPath = std::string(home) + "/.config/willow/context.json";
        loadContextConfig(contextPath);
    }
}

void CommandExecutor::applyDefaultContext() {
    m_context.defaultApps = {
        {"browser", "firefox"},
        {"terminal", "kgx"},
        {"file_manager", "nautilus"},
    };
    m_context.searchEngines = {
        {"youtube", "https://www.youtube.com/results?search_query="},
        {"google", "https://www.google.com/search?q="},
        {"facebook", "https://www.facebook.com/search/top?q="},
        {"reddit", "https://www.reddit.com/search/?q="},
        {"wikipedia", "https://en.wikipedia.org/wiki/Special:Search?search="},
        {"github", "https://github.com/search?q="},
    };
    m_context.appAliases = {
        {"browser", {"firefox", "chromium", "google-chrome", "brave-browser"}},
        {"spotify", {"spotify"}},
        {"vscode", {"code", "code-oss", "vscodium"}},
    };
}

bool CommandExecutor::openUrl(const std::string& url) {
    log("INFO", "Opening URL: " + url);
    const int result = spawnProcess({
        "systemd-run", "--user", "--scope", "--slice=app.slice", "--",
        "xdg-open", url
    });
    return result == 0;
}

void CommandExecutor::executeCommand(const std::string& command) {
    log("INFO", "Executing command: " + command);

    std::vector<std::string> args = {
        "systemd-run", "--user", "--scope", "--slice=app.slice", "--"
    };

    const auto commandParts = splitCommand(command);
    args.insert(args.end(), commandParts.begin(), commandParts.end());

    log("INFO", "Spawning: systemd-run for command");
    const int result = spawnProcess(args);

    if (result == 0) {
        log("INFO", "Command executed successfully");
    } else {
        log("ERROR", "Command execution failed with code: " + std::to_string(result));
    }
}

void CommandExecutor::typeText(const std::string& text) {
    if (text.empty()) return;
    
    if (!m_ydotoolAvailable) {
        log("ERROR", "ydotool is not available");
        return;
    }
    
    log("INFO", "Typing text: " + text);
    
    const int result = spawnProcess({"ydotool", "type", text});
    if (result != 0) {
        log("ERROR", "Failed to type text via ydotool");
    }
}

void CommandExecutor::pressKey(const std::string& keyCode) {
    if (!m_ydotoolAvailable) {
        log("ERROR", "ydotool is not available");
        return;
    }
    
    spawnProcess({"ydotool", "key", keyCode});
}

void CommandExecutor::pressKeyCombo(const std::vector<std::string>& keyCodes) {
    if (!m_ydotoolAvailable) {
        log("ERROR", "ydotool is not available");
        return;
    }
    
    std::vector<std::string> args = {"ydotool", "key"};
    args.insert(args.end(), keyCodes.begin(), keyCodes.end());
    spawnProcess(args);
}

double CommandExecutor::matchPhrase(const std::string& text, const std::string& phrase) {
    std::string lowerPhrase = phrase;
    std::transform(lowerPhrase.begin(), lowerPhrase.end(), lowerPhrase.begin(), ::tolower);
    
    if (text.find(lowerPhrase) != std::string::npos) {
        return 1.0;
    }

    const int distance = levenshteinDistance(text, lowerPhrase);
    const size_t maxLen = std::max(text.size(), lowerPhrase.size());
    if (maxLen == 0) {
        return 0.0;
    }

    const double similarity = 1.0 - (static_cast<double>(distance) / maxLen);
    const double overlap = tokenOverlapScore(text, lowerPhrase);
    return std::max(similarity, overlap);
}

std::pair<const Command*, double> CommandExecutor::findBestMatch(
    const std::string& text,
    const std::vector<Command>& commands,
    double threshold
) {
    const Command* bestCmd = nullptr;
    double bestConfidence = 0.0;
    
    for (const auto& cmd : commands) {
        for (const auto& phrase : cmd.phrases) {
            double confidence = matchPhrase(text, phrase);
            if (confidence > bestConfidence) {
                bestConfidence = confidence;
                bestCmd = &cmd;
            }
        }
    }
    
    if (bestConfidence < threshold) {
        return {nullptr, bestConfidence};
    }

    return {bestCmd, bestConfidence};
}

void CommandExecutor::log(const std::string& level, const std::string& message) {
    std::lock_guard<std::mutex> lock(m_logMutex);
    
    auto now = std::time(nullptr);
    auto tm = *std::localtime(&now);
    
    std::ofstream logFile(m_logFile, std::ios::app);
    if (logFile.is_open()) {
        logFile << std::put_time(&tm, "%Y-%m-%d %H:%M:%S") 
                << " [" << level << "] " << message << std::endl;
    }
    
    std::cout << "[" << level << "] " << message << std::endl;
}

bool CommandExecutor::executeSystemCommand(const std::string& command) {
    return spawnShellCommand(command) == 0;
}

bool CommandExecutor::isYdotoolAvailable() {
    return spawnShellCommand("which ydotool >/dev/null 2>&1") == 0;
}

std::string CommandExecutor::escapeForShell(const std::string& str) {
    std::string escaped;
    for (char c : str) {
        if (c == '\'') {
            escaped += "'\\''";
        } else {
            escaped += c;
        }
    }
    return escaped;
}

void CommandExecutor::loadContextConfig(const std::string& contextPath) {
    log("INFO", "Loading context config from: " + contextPath);
    
    std::ifstream file(contextPath);
    if (!file.is_open()) {
        log("WARNING", "Could not open context config file, using defaults");
        return;
    }
    
    Json::Value root;
    Json::CharReaderBuilder reader;
    std::string errors;
    
    if (!Json::parseFromStream(reader, file, &root, &errors)) {
        log("ERROR", "Failed to parse context config: " + errors);
        return;
    }
    
    if (root.isMember("default_apps") && root["default_apps"].isObject()) {
        for (const auto& key : root["default_apps"].getMemberNames()) {
            m_context.defaultApps[key] = root["default_apps"][key].asString();
        }
    }
    
    if (root.isMember("search_engines") && root["search_engines"].isObject()) {
        for (const auto& key : root["search_engines"].getMemberNames()) {
            m_context.searchEngines[key] = root["search_engines"][key].asString();
        }
    }
    
    if (root.isMember("app_aliases") && root["app_aliases"].isObject()) {
        for (const auto& key : root["app_aliases"].getMemberNames()) {
            std::vector<std::string> aliases;
            const Json::Value& aliasArray = root["app_aliases"][key];
            if (aliasArray.isArray()) {
                for (const auto& alias : aliasArray) {
                    aliases.push_back(alias.asString());
                }
            }
            m_context.appAliases[key] = aliases;
        }
    }
    
    log("INFO", "Context config loaded successfully");
}

bool CommandExecutor::isCommandAvailable(const std::string& command) {
    std::string cmdName = command;
    size_t spacePos = cmdName.find(' ');
    if (spacePos != std::string::npos) {
        cmdName = cmdName.substr(0, spacePos);
    }
    
    return spawnShellCommand("which " + cmdName + " >/dev/null 2>&1") == 0;
}

std::string CommandExecutor::findApp(const std::string& appName) {
    std::string lowerName = appName;
    std::transform(lowerName.begin(), lowerName.end(), lowerName.begin(), ::tolower);
    
    if (isCommandAvailable(lowerName)) {
        return lowerName;
    }
    
    if (m_context.appAliases.count(lowerName)) {
        for (const auto& alias : m_context.appAliases[lowerName]) {
            if (isCommandAvailable(alias)) {
                return alias;
            }
        }
    }
    
    if (m_context.defaultApps.count(lowerName)) {
        std::string defaultApp = m_context.defaultApps[lowerName];
        if (isCommandAvailable(defaultApp)) {
            return defaultApp;
        }
    }
    
    return "";
}

std::string CommandExecutor::urlEncode(const std::string& str) {
    std::ostringstream encoded;
    encoded.fill('0');
    encoded << std::hex;
    
    for (char c : str) {
        if (std::isalnum(c) || c == '-' || c == '_' || c == '.' || c == '~') {
            encoded << c;
        } else if (c == ' ') {
            encoded << '+';
        } else {
            encoded << '%' << std::setw(2) << int(static_cast<unsigned char>(c));
        }
    }
    
    return encoded.str();
}

bool CommandExecutor::executeSmartOpen(const std::string& appName) {
    log("INFO", "Smart open requested for: " + appName);
    
    std::string command = findApp(appName);
    
    if (command.empty()) {
        log("WARNING", "Application not found: " + appName);
        return false;
    }
    
    log("INFO", "Opening application: " + command);
    executeCommand(command);
    return true;
}

bool CommandExecutor::executeSmartSearch(const std::string& engine, const std::string& query) {
    log("INFO", "Smart search requested - Engine: " + engine + ", Query: " + query);

    std::string lowerEngine = engine;
    std::transform(lowerEngine.begin(), lowerEngine.end(), lowerEngine.begin(), ::tolower);

    if (m_context.searchEngines.count(lowerEngine) == 0) {
        log("WARNING", "Unknown search engine: " + engine);
        return false;
    }

    std::string baseUrl = m_context.searchEngines[lowerEngine];
    std::string encodedQuery = urlEncode(query);
    std::string url = baseUrl + encodedQuery;

    const bool ok = openUrl(url);
    if (ok) {
        log("INFO", "Search URL opened: " + url);
    }
    return ok;
}

} // namespace VoiceAssistant
