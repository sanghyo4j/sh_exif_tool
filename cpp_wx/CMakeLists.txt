cmake_minimum_required(VERSION 3.16)
project(ExifToolWxApp)

set(CMAKE_CXX_STANDARD 17)

find_package(wxWidgets REQUIRED COMPONENTS core base)
include(${wxWidgets_USE_FILE})

include_directories(include)
add_executable(ExifTool
    src/App.cpp
    src/MainFrame.cpp
)

target_link_libraries(ExifTool ${wxWidgets_LIBRARIES})
