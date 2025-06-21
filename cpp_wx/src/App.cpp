#include "App.h"
#include "MainFrame.h"

wxIMPLEMENT_APP(App);

bool App::OnInit() {
    auto* frame = new MainFrame("EXIF Tool", wxDefaultPosition, wxSize(800, 600));
    frame->Show(true);
    return true;
}
