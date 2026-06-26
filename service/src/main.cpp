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
    // Set up signal handlers
    std::signal(SIGINT, signalHandler);
    std::signal(SIGTERM, signalHandler);

    try {
        std::cout << "Starting Voice Assistant Service..." << std::endl;

        // Create D-Bus connection
        auto connection = sdbus::createSessionBusConnection();
        
        // Request D-Bus name
        connection->requestName(sdbus::ServiceName{"com.github.saim.Willow"});
        
        // Create service object
        const std::string objectPath = "/com/github/saim/VoiceAssistant";
        VoiceAssistant::VoiceAssistantService service(*connection, objectPath);

        std::cout << "Willow Service running on D-Bus" << std::endl;
        std::cout << "Bus name: com.github.saim.Willow" << std::endl;
        std::cout << "Object path: " << objectPath << std::endl;
        std::cout << "Press Ctrl+C to exit" << std::endl;

        // Run the event loop
        while (!g_exitRequested) {
            connection->processPendingEvent();
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }

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
