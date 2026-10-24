#![no_std]

// We use Box and other types that require the heap, so we have to include this
extern crate alloc;

use rand::RngExt;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::{future::Future, pin::Pin};

use heapless::Vec as FixedVec;
use xpanse_api::{
    app::App,
    interfaces::buttons::{A, Button},
    reexports::{
        defmt,
        embassy_futures::select::{Either, select},
        embassy_time::{Duration, Ticker, Timer, Instant},
        slint::{self, ComponentHandle, Model, ModelRc, VecModel},
    },
    registry::{Registry, ResourceLease},
};

// This will include all the components that we define in `.slint` files
slint::include_modules!();

const LEVEL_SPEED_TICKS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(50),
    Duration::from_millis(30),
];
const BIRD_X: i16 = 40;
const SPAWN_X: i16 = 330;
const SPAWN_INTERVAL: u32 = 35;
const GRAVITY: i16 = 1;
const JUMP_SPEED: i16 = 7;
const FLAP_TICKS: u32 = 3;
const DEATH_FLOOR: i16 = 180;
const PIPE_WIDTH: i16 = 46;
const PIPE_GAP: i16 = 80;
const MAX_PIPES: usize = 4;
const PIPE_SPEED: i16 = 5;
const MENU_FLAP_TICKS: Duration = Duration::from_millis(200);

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
            ui.set_tubes(ModelRc::from(pipe_model.clone()));


            // We try to show this component on the screen
            if ui.show().is_err() {
                defmt::error!("FlappyBird: failed to show UI");
                return;
            }


            let button = self.button.resource_mut();
            let mut ticker_flap = Ticker::every(MENU_FLAP_TICKS);
            let mut tick_flap = ticker_flap.next();

            // The loop that is the selection menu
            'select: loop {
                let mut button_pressed_menu = button.wait_for_pressed();
                let _select = match select(
                    &mut button_pressed_menu,
                    &mut tick_flap,
                )
                .await
                {
                    Either::First(()) => {
                        // We count how many milliseconds the button was held
                        drop(button_pressed_menu);
                        let mut held_ms = 0;
                        while button.is_pressed() && held_ms < 500 {
                            Timer::after_millis(20).await;
                            held_ms += 20;
                            if held_ms >= 500 {
                                ui.set_selected(true);
                            }
                        }
                    
                        // If the button was held for more than 1000 ms, we exit from the selection menu
                        if held_ms >= 500 {
                            while button.is_pressed() {
                                Timer::after_millis(20).await;
                            }
                            if game.level_select == 3 {
                                game.low_grapics = !game.low_grapics;
                                ui.set_low_grapics(game.low_grapics);
                                ui.set_selected(false);
                            } else {
                                break 'select;
                            }
                        } else {
                            // If the press was shorter than 1,000 ms, we move to the next level difficulty
                            // If we are at the last level, we go back to the beginning
                            if game.level_select > 2 {
                                game.level_select = 0;
                            } else {
                                game.level_select += 1;
                            }
                        }
                    },
                    Either::Second(()) => {
                        drop(tick_flap);
                        drop(button_pressed_menu);
                        game.bird_flap = !game.bird_flap;
                        tick_flap = ticker_flap.next();
                    }
                };

                // Then update the ui aka the frontend
                update_ui_bird(&ui, &game);
                ui.set_level(game.level_select as i32);
                
                // Finally retake the button to wait for a press
                button_pressed_menu = button.wait_for_pressed();
            }

            // Then start the game up
            // First turn off the level select menu
            ui.set_level_select(false);

            let mut button_pressed = button.wait_for_pressed();
            let mut jumping: bool;
            let mut been_killed = false;
            let mut ticker = Ticker::every(LEVEL_SPEED_TICKS[game.level_select]);
            let mut tick = ticker.next();

            // Then start the game loop
            'game: loop {
                let _jumped = match select(
                    &mut button_pressed,
                    &mut tick,
                )
                .await
                {
                    Either::First(()) => jumping = true,
                    Either::Second(()) => {
                        drop(tick);
                        been_killed = game.advance();
                        sync_pipe_model(&pipe_model, &game);
                        if game.has_died() {
                            been_killed = true;
                        }
                        tick = ticker.next();
                        jumping = false;
                    }
                };

                drop(button_pressed);

                // If you are jumping tell the game to go up
                if jumping {
                    game.jump();
                }

                update_ui_bird(&ui, &game);
                update_ui_info(&ui, &game);

                if been_killed {
                    break 'game;
                }

                button_pressed = button.wait_for_pressed();
            }

            'game_over: loop {
                ui.set_selected(false);
                ui.set_has_died(true);
                let mut held_ms = 0;
                while button.is_pressed() && held_ms < 1_000 {
                    Timer::after_millis(20).await;
                    held_ms += 20;
                    if held_ms >= 500 {
                        ui.set_selected(true);
                    }
                }

                // If the button was held for more than 1000 ms, we exit from the death screen
                if held_ms >= 500 {
                    while button.is_pressed() {
                        Timer::after_millis(20).await;
                    }
                    break 'game_over;
                }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MovingPipe {
    x: i16,
    y: i16,
}

// All the vallues from the game
struct Game {
    pipes: FixedVec<MovingPipe, MAX_PIPES>,
    count: u32,
    level_select: usize,
    bird_y: i16,
    bird_speed: i16,
    bird_falling: bool,
    bird_flap_tick: u32,
    bird_flap: bool,
    tick: u32,
    low_grapics: bool,
    rng: SmallRng,
}

impl Game {
    // The starting logic
    fn new() -> Self {
        let mut game = Self {
            pipes: FixedVec::new(),
            count: 0,
            level_select: 0,
            bird_y: 100,
            bird_speed: 0,
            bird_flap_tick: 0,
            bird_flap: false,
            bird_falling: false,
            tick: 0,
            low_grapics: true,
            rng: SmallRng::seed_from_u64(Instant::now().as_micros()),
        }; 

        game.spawn_pipe();
        game
    }

    // The logic when you press the bird to go up
    fn jump(&mut self) {
        self.bird_speed = -JUMP_SPEED;
        self.bird_flap_tick = FLAP_TICKS;
    }

    // Advance the game by 1 in game tick
    fn advance(&mut self) -> bool{
        self.tick = self.tick.wrapping_add(1);
        if self.bird_flap_tick > 0 {
            self.bird_flap_tick -= 1;
            self.bird_flap = !self.bird_flap;
        } else {
            self.bird_flap = false;
        }

        // Advance the pipes by the amount specified in pipe_speed
        for pipe in &mut self.pipes {
            pipe.x -= PIPE_SPEED;
        }

        // Update the birds posistion with the speed calculations
        self.bird_speed = (self.bird_speed + GRAVITY).clamp(-JUMP_SPEED, JUMP_SPEED);
        self.bird_y = (self.bird_y + self.bird_speed).max(0);
        self.bird_falling = self.bird_speed > 0;

        // Check if the bird died from a collision with a pipe
        let mut died = false;
        for pipe in &mut self.pipes {
            // Check if the pipe is in the range of the bird on the x axis
            if (pipe.x < BIRD_X + PIPE_WIDTH / 2) && (pipe.x > BIRD_X - PIPE_WIDTH / 2) {
                // Then check if it is touching one of the pipes
                if (self.bird_y > pipe.y + (PIPE_GAP / 2)) || (self.bird_y < (pipe.y - PIPE_GAP / 2)) {
                    died = true;
                }
            }
        }

        // Check if there has gone a pipe of screen
        let mut index = 0;
        while index < self.pipes.len() {
            if self.pipes[index].x < -40 {
                self.pipes.remove(index);
                self.add_point();
            } else {
                index += 1;
            }
        }

        // Then check if the bird hit the ground
        if self.has_died() {
            died = true;
        }

        if self.tick.is_multiple_of(SPAWN_INTERVAL) && !self.has_died() {
            self.spawn_pipe()
        }

        died
    }

    const fn has_died(&self) -> bool {
        self.bird_y > DEATH_FLOOR
    }

    // The function for spawning in new pipes
    fn spawn_pipe(&mut self) {
        let spawn_y: i16 = self.rng.random_range(20..181);
        let _ = self.pipes.push(MovingPipe { x: SPAWN_X, y: spawn_y });
    }

    // A function for adding a point when a pipe goes away
    fn add_point(&mut self) {
        self.count += 1;
    }

    // Idk yet what this does :skulk:
    fn ui_pipes(&self) -> Vec<GamePipe> {
        self.pipes
            .iter()
            .map(|pipe| GamePipe {
                x_pos: i32::from(pipe.x),
                top_y_pos: i32::from(pipe.y) - i32::from(PIPE_GAP) / 2,
                bot_y_pos: i32::from(pipe.y) + i32::from(PIPE_GAP) / 2,
            })
            .collect()
    }
}

// This function syncs the pipes that are in the backend to the frontend
fn sync_pipe_model(model: &VecModel<GamePipe>, game: &Game) {
    while model.row_count() > game.pipes.len() {
        model.remove(model.row_count() - 1);
    };
    for (index, pipe) in game.pipes.iter().enumerate() {
        let ui_pipe = GamePipe {
            x_pos: i32::from(pipe.x),
            top_y_pos: i32::from(pipe.y) - i32::from(PIPE_GAP) / 2,
            bot_y_pos: i32::from(pipe.y) + i32::from(PIPE_GAP) / 2,
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
    ui.set_count(game.count.min(i32::MAX as u32) as i32);
}

// Update the bird place and orientation
fn update_ui_bird(ui: &FlappyBirdUI, game: &Game) {
    ui.set_bird_y(i32::from(game.bird_y));
    ui.set_bird_falling(game.bird_falling);
    ui.set_bird_flapping(game.bird_flap);
}