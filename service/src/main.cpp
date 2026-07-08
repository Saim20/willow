#include "VoiceAssistantService.hpp"
#include <sdbus-c++/sdbus-c++.h>
#include <iostream>
#include <csignal>
#include <atomic>
#include <thread>
#include <chrono>

std::atomic<bool> g_exitRequested{false};

void signalHandler(int signal) {
    if (signal == SIGINT || signal == SIGTERM) {
        std::cout << "\nShutting down Voice Assistant Service..." << std::endl;
        g_exitRequested = true;
    }
}

int main(int /*argc*/, char* /*argv*/[]) {
    std::signal(SIGINT, signalHandler);
    std::signal(SIGTERM, signalHandler);

    try {
        std::cout << "Starting Voice Assistant Service..." << std::endl;

        auto connection = sdbus::createSessionBusConnection();
        connection->requestName(sdbus::ServiceName{"com.github.saim.Willow"});
        
        const std::string objectPath = "/com/github/saim/VoiceAssistant";
        VoiceAssistant::VoiceAssistantService service(*connection, objectPath);

        std::cout << "Willow Service running on D-Bus" << std::endl;
        std::cout << "Bus name: com.github.saim.Willow" << std::endl;
        std::cout << "Object path: " << objectPath << std::endl;
        std::cout << "Press Ctrl+C to exit" << std::endl;

        connection->enterEventLoopAsync();

        while (!g_exitRequested) {
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }

        connection->leaveEventLoop();

        std::cout << "Service stopped successfully" << std::endl;
        return 0;

    } catch (const sdbus::Error& e) {
        std::cerr << "D-Bus error: " << e.what() << std::endl;
        return 1;
    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
}
