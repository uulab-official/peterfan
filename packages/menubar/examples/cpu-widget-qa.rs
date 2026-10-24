//! Isolated native CPU-item QA. Never loads/saves settings, installs a helper,
//! or opens the PeterFan app. Writes evidence only to the supplied directory.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/cpu_widget.rs"]
mod cpu_widget;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/native_cpu_widget.rs"]
mod native_cpu_widget;

#[cfg(target_os = "macos")]
fn main() {
    use cpu_widget::CpuWidgetHistory;
    use native_cpu_widget::NativeCpuWidget;
    use peterfan_core::config::{CpuWidgetConfig, CpuWidgetStyle, ResolvedLanguage};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };
    use tao::{
        event_loop::{ControlFlow, EventLoopBuilder},
        platform::{
            macos::{ActivationPolicy, EventLoopExtMacOS},
            run_return::EventLoopExtRunReturn,
        },
    };
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .expect("supply a private output directory");
    std::fs::create_dir_all(&output).unwrap();
    let mut event_loop = EventLoopBuilder::<()>::new().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let mut config = CpuWidgetConfig {
        enabled: true,
        ..CpuWidgetConfig::default()
    };
    let mut history = CpuWidgetHistory::default();
    history.push(50.);
    let mut widget = None;
    let mut phase = 0;
    let mut next = Instant::now();
    let mut snapshots = Vec::new();
    let mut failure = None;
    event_loop.run_return(|_,_,flow| {
        if Instant::now()<next { *flow=ControlFlow::WaitUntil(next);return; }
        let result=(|| -> Result<(),String> {
            if widget.is_none() {
                let staged=NativeCpuWidget::new(config,ResolvedLanguage::En)?;
                let item=staged.tray().ns_status_item().ok_or("hidden staging removed the native item")?;
                if item.isVisible(){return Err("staged item was visible before commit".into());}
                widget=Some(staged);
            }
            let active=widget.as_mut().unwrap();
            match phase {
                0=>{},
                1=>{config.show_percentage=false;history.push(0.);},
                2=>{config.style=CpuWidgetStyle::History;config.show_percentage=true;history.push(100.);},
                3=>{config.show_percentage=false;history.push(f32::NAN);},
                4=>{config.style=CpuWidgetStyle::Number;history.push(100.);},
                _=>{active.set_visible(false)?;if active.tray().ns_status_item().is_none(){return Err("hiding destroyed the owned native item".into());}return Ok(());},
            }
            active.update(config,ResolvedLanguage::En,&history)?;active.set_visible(true)?;
            let item=active.tray().ns_status_item().ok_or("missing active native item")?;
            let marker=objc2_foundation::MainThreadMarker::new().ok_or("not on main thread")?;
            let button=item.button(marker).ok_or("missing native button")?;
            if !item.isVisible() || (item.length()-cpu_widget::point_width(config)).abs()>0.1 {return Err("native visibility/width mismatch".into());}
            snapshots.push(serde_json::json!({"style":config.style,"percentage":config.show_percentage,"width":item.length(),"title":button.title().to_string(),"image_present":button.image().is_some(),"button_width":button.bounds().size.width,"rect":format!("{:?}",active.tray().rect())}));
            Ok(())
        })();
        if let Err(error)=result {failure=Some(error);*flow=ControlFlow::Exit;return;}
        phase+=1;
        if phase>5 {*flow=ControlFlow::Exit;}else{next=Instant::now()+Duration::from_millis(600);*flow=ControlFlow::WaitUntil(next);}
    });
    drop(widget);
    let passed = failure.is_none() && snapshots.len() == 5;
    std::fs::write(output.join("native-lifecycle.json"),serde_json::to_vec_pretty(&serde_json::json!({"passed":passed,"error":failure,"snapshots":snapshots,"scope":"isolated native item; no installed-app/config/helper operations"})).unwrap()).unwrap();
    if !passed {
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("CPU widget native QA requires macOS");
    std::process::exit(2);
}
