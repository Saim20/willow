#pragma once

#include <string>
#include <vector>
#include <mutex>
#include <map>
#include <json/json.h>

namespace VoiceAssistant {

struct Command {
    std::string name;
    std::string command;
    std::vector<std::string> phrases;
};

struct ContextConfig {
    std::map<std::string, std::string> defaultApps;
    std::map<std::string, std::string> searchEngines;
    std::map<std::string, std::vector<std::string>> appAliases;
};

/**
 * CommandExecutor - Common core for executing commands and simulating keystrokes
 * Shared by all mode workers to avoid duplication
 */
class CommandExecutor {
public:
    CommandExecutor();
    ~CommandExecutor() = default;

    // Command execution
    void executeCommand(const std::string& command);
    
    // Smart workflows
    bool executeSmartOpen(const std::string& appName);
    bool executeSmartSearch(const std::string& engine, const std::string& query);
    bool openUrl(const std::string& url);
    
    // Keyboard simulation via ydotool
    void typeText(const std::string& text);
    void pressKey(const std::string& keyCode);
    void pressKeyCombo(const std::vector<std::string>& keyCodes);
    
    // Command matching
    double matchPhrase(const std::string& text, const std::string& phrase);
    std::pair<const Command*, double> findBestMatch(
        const std::string& text,
        const std::vector<Command>& commands,
        double threshold
    );
    
    // Context configuration
    void loadContextConfig(const std::string& contextPath);
    void applyDefaultContext();
    const ContextConfig& getContextConfig() const { return m_context; }
    
    // Logging
    void log(const std::string& level, const std::string& message);

private:
    std::string m_logFile;
    mutable std::mutex m_logMutex;
    ContextConfig m_context;
    bool m_ydotoolAvailable;
    
    // Helper for command execution
    bool executeSystemCommand(const std::string& command);
    
    // Smart workflow helpers
    bool isCommandAvailable(const std::string& command);
    std::string findApp(const std::string& appName);
    std::string urlEncode(const std::string& str);
    
    // Helper for ydotool operations
    bool isYdotoolAvailable();
    std::string escapeForShell(const std::string& str);
};

} // namespace VoiceAssistant
