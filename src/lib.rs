#![no_std]

// We use Box and other types that require the heap, so we have to include this
extern crate alloc;

use rand::Rng;
use alloc::boxed::Box;
use core::{future::Future, pin::Pin};

use heapless::Vec as FixedVec;
use xpanse_api::{
    app::App,
    interfaces::buttons::{A, Button},
    reexports::{
        defmt,
        embassy_time::Timer,
        slint::{self, ComponentHandle},
    },
    registry::{Registry, ResourceLease},
};

// This will include all the components that we define in `.slint` files
slint::include_modules!();

const START_TICK_INTERVAL: u16 = 100;
const TICK_SPEEDUP: u16 = 1;
const SPAWN_X: u8 = 330;
const SPAWN_INTERVAL: u16 = 10;
const GRAVITY: f32 = 9.81;
const JUMP_SPEED: f32 = 10;
const DEATH_FLOOR: u8 = 180;
const PIPE_WIDTH: u8 = 46;
const PIPE_GAP: u8 = 80;
const MAX_PIPES: u8 = 3;

// Every app is a struct that has all the resources that it needs inside it
pub struct FlappyBirdApp {
    button: ResourceLease<Box<dyn Button<A>>>,
}

// Every app needs to implement the App trait
impl App for FlappyBirdApp {
    // This will be the display name in the app list
    const NAME: &'static str = "Flappy Bird";

    // This function checks if the registry has all
    // the resources that this app *needs* to run, like controls/buttons.
    //
    // And if it can run, it will be added to the app list
    fn can_run(registry: &Registry) -> bool {
        // We check if the registry has an A button
        registry.has::<Box<dyn Button<A>>>()
    }

    // This function takes all the resources that the app needs
    // and creates a new app instance if everything is successful
    //
    // This function can try to take resources that the app
    // doesn't *need* to run. For example, a game can run without sound,
    // but not without buttons.
    fn new(registry: &mut Registry) -> Option<Self> {
        // We try to take an A button
        let button = registry.take_resource::<Box<dyn Button<A>>>()?;
        Some(Self { button })
    }

    // This is where the actual app runs
    fn run<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
        // We need a Box because of Rust's async trait requirements
        Box::pin(async move {
            // We try to create an instance of our exported component
            let ui = match FlappyBirdUI::new() {
                Ok(ui) => ui,
                Err(_) => {
                    defmt::error!("FlappyBird: failed to create UI");
                    return;
                }
            };

            let mut game = Game::new();
            let pipe_model = Rc::new(VecModel::from(game.ui_pipes()));

            // We try to show this component on the screen
            if ui.show().is_err() {
                defmt::error!("FlappyBird: failed to show UI");
                return;
            }

            // The loop that is the selection menu
            'select: loop {
                // We wait until the button is pressed
                self.button.resource_mut().wait_for_pressed().await;

                // We count how many milliseconds the button was held
                let mut held_ms = 0;
                while self.button.resource().is_pressed() && held_ms < 1_000 {
                    Timer::after_millis(20).await;
                    held_ms += 20;
                }

                // If the button was held for more than 1000 ms, we exit from the selection menu
                if held_ms >= 1_000 {
                    while self.button.resource().is_pressed() {
                        Timer::after_millis(20).await;
                    }
                    break 'select;
                }

                // If the press was shorter than 1,000 ms, we move to the next level difficulty
                // If we are at the last level, we go back to the beginning
                if Self.level_select > 1 {
                    Self.level_select = 0;
                } else {
                    Self.level_select += 1;
                }

                // Slint automatically generates `set_` functions for reactive variables
                ui.set_level(Self.level_select as i32);
            }

            // Then start the game up
            // First turn off the level select menu
            ui.set_level_select(false);

            // Then idk
            'game: loop {
                self.button.resource_mut().wait_for_pressed().await;
                
            }

            // Hide the UI when the app exits
            if ui.hide().is_err() {
                defmt::error!("FlappyBird: failed to hide UI");
            }
        })
    }

    // Since we took a button from the registry,
    // we should also give it back
    fn release(self, registry: &mut Registry) {
        registry.return_resource(self.button);
    }
}

// The struct where the pipes that come to you are stored
struct FallingPipe {
    x: u8,
    y: u8,
}

// All the vallues from the game
struct Game {
    pipes: FixedVecL<FallingPipe, MAX_PIPES>
    count: u32,
    bird_y: u16,
    bird_falling: bool,
    tick_speed: u16,
    tick: u32,
    level_select: u8,
}

impl Game {
    fn new() -> Self {
        let mut game = Self {
            pipes: FixedVec::new(),
            count: 0,
            bird_y: 100,
            bird_falling: false,
            tick_speed: START_TICK_INTERVAL,
            tick: 0,
            level_select: 0,
        };
        game.spawn_tube();
        game
    }

    // The function for spawning in new notes
    fn spawn_tube(&mut self) {
        let y: u8 = rng.genrange(50..201);
        let _ = self.pipes.push(FallingPipe { x: SPAWN_X, y });
    }

    // Idk yet what this does :skulk:
    fn ui_pipes(&self) -> Vec<GamePipe> {
        self.pipes
            .iter()
            .map(|pipe| GamePipe {
                x: i32::from(pipe.x),
                y: i32::from(pipe.y),
            })
            .collect()
    }
}

fn sync_pipe_model(model: &VecModel<GamePipe>, game: &Game) {
    while model.row_count() > game.pipes.len() {
        
    };
    for (index, pipe) in game.pipes.iter().enumerate() {
        let ui_pipe = GamePipe {
            x: i32::from(note.x),
            y: i32::from(note.y),
        };
        if index < model.row_count() {
            model.set_row_data(index, ui_pipe);
        } else {
            model.push(ui_pipe);
        }
    }
}

// Update the games ui info from the backend
fn update_ui_info(ui: &FlappyBirdUI, game: &Game) {
    ui.set_count(i32::from(game.count));
}