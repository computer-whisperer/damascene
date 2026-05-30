#import <UIKit/UIKit.h>

extern void start_winit_app(void);

int main(int argc, char *argv[]) {
    @autoreleasepool {
        (void)argc;
        (void)argv;
        start_winit_app();
    }
    return 0;
}
