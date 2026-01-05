//! Quick check for GPU acceleration

#[cfg(feature = "metal")]
use candle_core::Device;

fn main() {
    println!("Checking GPU acceleration...\n");

    // Check Metal
    #[cfg(feature = "metal")]
    {
        print!("Metal feature: ENABLED\n");
        match Device::new_metal(0) {
            Ok(device) => {
                println!("Metal device: AVAILABLE");
                println!("Device info: {:?}", device);
            }
            Err(e) => {
                println!("Metal device: FAILED - {}", e);
            }
        }
    }

    #[cfg(not(feature = "metal"))]
    {
        println!("Metal feature: DISABLED");
    }

    println!("\nCPU is always available as fallback");
}
