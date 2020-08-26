mod stream;
mod server;
mod timed_udp;

use server::ServerInfo;

const GET_SERVERS: druid::Selector<()> = druid::Selector::new("GET_SERVERS");
const STOP_SERVERS: druid::Selector<()> = druid::Selector::new("STOP_SERVERS");
const RECEIVE_SERVER: druid::Selector<ServerInfo> = druid::Selector::new("RECEIVE_SERVER");
const STOPPED: druid::Selector<()> = druid::Selector::new("STOPPED");

#[derive(Clone, druid::Data, druid::Lens)]
struct State {
    servers: druid::im::Vector<String>,
    loading: bool,
}

struct Delegate {
    sink: druid::ExtEventSink,
    task: Option<std::thread::JoinHandle<()>>,
}

pub fn send_server(sink: druid::ExtEventSink, payload: ServerInfo) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        sink.submit_command(RECEIVE_SERVER, payload, druid::Target::Global).unwrap();
    })
}

pub fn send_kill(sink: druid::ExtEventSink) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        sink.submit_command(STOP_SERVERS, (), druid::Target::Global).unwrap();
    })
}

async fn receiver_task(sink: druid::ExtEventSink, mut receiver: tokio::sync::mpsc::UnboundedReceiver<server::ServerInfo>) {
    while let Some(server) = receiver.recv().await {
        send_server(sink.clone(), server);
    }
}

fn get_servers(sink: druid::ExtEventSink) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut rt = tokio::runtime::Runtime::new().unwrap();
        let _: std::io::Result<()> = rt.block_on(async move {
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            // TODO: proper error handling
            let send_task = tokio::task::spawn(stream::query_master(sender));
            let recv_task = tokio::task::spawn(receiver_task(sink.clone(), receiver));

            send_task.await??;
            recv_task.await?;
            send_kill(sink.clone()).await?;

            Ok(())
        });
    })
}

impl druid::AppDelegate<State> for Delegate {
    fn command(&mut self, _: &mut druid::DelegateCtx, _: druid::Target, cmd: &druid::Command, state: &mut State, _: &druid::Env) -> bool {
        if cmd.is(GET_SERVERS) && self.task.is_none() {
            self.task = Some(get_servers(self.sink.clone()));
            state.servers = druid::im::vector![];
            state.loading = true;
        } else if let Some(server) = cmd.get(RECEIVE_SERVER) {
            state.servers.push_back(server.name.clone());
        } else if cmd.is(STOP_SERVERS) && self.task.is_some() {
            let handle = self.task.take().unwrap();
            let sink = self.sink.clone();
            std::thread::spawn(move || {
                let _ = handle.join();
                sink.submit_command(STOPPED, (), druid::Target::Global).unwrap();
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

fn main() -> Result<(), druid::PlatformError> {
    use druid::{AppLauncher, WindowDesc, LocalizedString, im::vector};
    let app = AppLauncher::with_window(WindowDesc::new(ui).title(LocalizedString::new("Mordhau Browser")));
    let delegate = Delegate { sink: app.get_external_handle(), task: None };
    app.delegate(delegate)
        .use_simple_logger()
        .launch(State { servers: vector![], loading: false })
}
