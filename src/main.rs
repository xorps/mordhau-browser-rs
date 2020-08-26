mod stream;
mod server;
mod timed_udp;

use tokio::task::{spawn, spawn_blocking};
use tokio::task::JoinHandle;

const GET_SERVERS: druid::Selector<()> = druid::Selector::new("GET_SERVERS");
const STOP_SERVERS: druid::Selector<()> = druid::Selector::new("STOP_SERVERS");
const RECEIVE_SERVER: druid::Selector<String> = druid::Selector::new("RECEIVE_SERVER");
const STOPPED: druid::Selector<()> = druid::Selector::new("STOPPED");

#[derive(Clone, druid::Data, druid::Lens)]
struct State {
    servers: druid::im::Vector<String>,
    loading: bool,
}

struct Delegate {
    sink: druid::ExtEventSink,
    task: Option<JoinHandle<()>>,
}

pub fn send_server(sink: druid::ExtEventSink, payload: server::ServerInfo) -> JoinHandle<()> {
    spawn_blocking(move || {
        let payload = payload.name;
        sink.submit_command(RECEIVE_SERVER, payload, druid::Target::Global).unwrap();
    })
}

fn get_servers(sink: druid::ExtEventSink) -> JoinHandle<()> {
    spawn(async move {
        // TODO: error handling
        stream::query_master(sink.clone()).await.unwrap();
        // send stop/kill signal
        let _ = spawn_blocking(move || {
            sink.submit_command(STOP_SERVERS, (), druid::Target::Global).unwrap();
        }).await;
    })
}

impl druid::AppDelegate<State> for Delegate {
    fn command(&mut self, _: &mut druid::DelegateCtx, _: druid::Target, cmd: &druid::Command, state: &mut State, _: &druid::Env) -> bool {
        if cmd.is(GET_SERVERS) && self.task.is_none() {
            self.task = Some(get_servers(self.sink.clone()));
            state.servers = druid::im::vector![];
            state.loading = true;
        } else if let Some(server) = cmd.get(RECEIVE_SERVER) {
            state.servers.push_back(server.clone());
        } else if cmd.is(STOP_SERVERS) && self.task.is_some() {
            let handle = self.task.take().unwrap();
            let sink = self.sink.clone();
            spawn(async move {
                // handle.cancel().await;
                let _ = handle.await;
                let _ = spawn_blocking(move || { 
                    sink.submit_command(STOPPED, (), druid::Target::Global).unwrap();
                }).await;
            });
        } else if cmd.is(STOPPED) {
            println!("Total Servers: {}", state.servers.len());
            state.loading = false;
        }
        // never propagate the tree
        false
    }
}

fn ui() -> impl druid::Widget<State> {
    use druid::{WidgetExt, widget::*};
    let get_servers = Button::new("Get Servers")
        .on_click(|ctx, _, _| ctx.submit_command(GET_SERVERS, druid::Target::Global))
        .align_right();
    let stop = Button::new("Stop")
        .on_click(|ctx, _, _| ctx.submit_command(STOP_SERVERS, druid::Target::Global))
        .align_right();
    let buttons = Either::new(|state: &State, _| state.loading, stop, get_servers);
    let scroll = Scroll::new(List::new(|| Label::new(|server: &String, _: &_| format!("{}", server))))
        .vertical()
        .lens(State::servers)
        .expand_width();
    let mut root = Flex::column();
    root.add_child(buttons);
    root.add_flex_child(scroll, FlexParams::new(1.0, CrossAxisAlignment::Start));
    root.padding(10.0)
}

#[tokio::main(threaded_scheduler)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(spawn_blocking(move || {
        use druid::{AppLauncher, WindowDesc, LocalizedString, im::vector};
        let app = AppLauncher::with_window(WindowDesc::new(ui).title(LocalizedString::new("Mordhau Browser")));
        let delegate = Delegate { sink: app.get_external_handle(), task: None };
        app.delegate(delegate)
            .use_simple_logger()
            .launch(State { servers: vector![], loading: false })
    }).await??)
}
