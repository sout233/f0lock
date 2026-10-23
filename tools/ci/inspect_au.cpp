#include <CoreFoundation/CoreFoundation.h>
#include <iostream>
#include <string>

int main(int argc, char **argv) {
    if (argc != 2) {
        std::cerr << "Pass the installed f0lock.component path\n";
        return 1;
    }

    const std::string path = argv[1];
    auto url = CFURLCreateFromFileSystemRepresentation(
        nullptr, reinterpret_cast<const UInt8 *>(path.data()), path.size(), true);
    auto bundle = CFBundleCreate(nullptr, url);
    CFRelease(url);
    if (!bundle) {
        std::cerr << "Cannot create CFBundle for " << path << '\n';
        return 1;
    }

    CFErrorRef error = nullptr;
    if (!CFBundleLoadExecutableAndReturnError(bundle, &error)) {
        if (error) {
            CFShow(error);
            CFRelease(error);
        }
        CFRelease(bundle);
        return 1;
    }
    auto factory = CFBundleGetFunctionPointerForName(bundle, CFSTR("wrapAsAUV2_inst0Factory"));
    if (!factory) {
        std::cerr << "The Audio Unit factory export is missing\n";
        CFRelease(bundle);
        return 1;
    }
    std::cout << "Loaded installed AU executable and resolved its factory export\n";
    CFRelease(bundle);
    return 0;
}
