-- Animate: the engine's default character animation.
--
-- A Space replaces it by putting its own script named Animate in
-- StarterPlayer.StarterCharacterScripts. Its four Animation children name the
-- avatar's own clips (rig://idle, rig://walk, rig://run, rig://jump), so the
-- same script plays every body option's clips and any *.rig.toml replacement.

local RunService = game:GetService("RunService")

local character = script.Parent
local humanoid = character:WaitForChild("Humanoid")
local animator = humanoid:WaitForChild("Animator")

local function load(name)
	local animation = script:WaitForChild(name)
	local track = animator:LoadAnimation(animation)
	track.Priority = Enum.AnimationPriority.Core
	track.Looped = true
	return track, animation:GetAttribute("AuthoredSpeed")
end

local idle = load("idle")
local walk, walkSpeed = load("walk")
local run, runSpeed = load("run")
local jump = load("jump")

-- Every track plays for the character's whole life and only its weight moves,
-- so a crossfade never starts from a track that is not playing.
idle:Play(0, 1)
walk:Play(0, 0)
run:Play(0, 0)
jump:Play(0, 0)

local speed, airborne, climbing = 0, false, false
humanoid.Running:Connect(function(s)
	speed = s
end)
humanoid.StateChanged:Connect(function(_, state)
	airborne = state == Enum.HumanoidStateType.Jumping or state == Enum.HumanoidStateType.Freefall
	climbing = state == Enum.HumanoidStateType.Climbing
end)

-- Changes are rounded to 1/32 so a steady gait sends nothing over the network.
local sent = {}
local function send(key, value, apply)
	value = math.floor(value * 32 + 0.5) / 32
	if sent[key] ~= value then
		sent[key] = value
		apply(value)
	end
end
local function weight(track, key, w)
	send(key, w, function(v)
		track:AdjustWeight(v, 0.1)
	end)
end
local function rate(track, key, r)
	send(key, r, function(v)
		track:AdjustSpeed(v)
	end)
end

RunService.Heartbeat:Connect(function()
	-- The gait follows the ground speed against the speed each clip was
	-- authored at, scaled by the body's stride, the way legs choose a gait: a
	-- Space's fast WalkSpeed plays the run, never a walk dragged across the
	-- ground. A clip without an authored speed falls back to the Humanoid's
	-- WalkSpeed and RunSpeed. Playback rate follows the same speeds.
	local stride = math.max(humanoid:GetAttribute("StrideScale") or 1, 0.25)
	local walkAt = math.max(if walkSpeed then walkSpeed * stride else humanoid.WalkSpeed, 0.05)
	local runAt = math.max(
		if runSpeed then runSpeed * stride else (humanoid:GetAttribute("RunSpeed") or walkAt * 2),
		walkAt + 0.05
	)
	local s = if climbing then 0 else speed

	local wi, ww, wr = 1, 0, 0
	if s >= 0.08 and s <= walkAt then
		ww = s / walkAt
		wi = 1 - ww
	elseif s > walkAt then
		wr = math.clamp((s - walkAt) / (runAt - walkAt), 0, 1)
		wi, ww = 0, 1 - wr
	end
	-- Climbing holds idle under the limb solve; the air takes the jump clip.
	local ground = if airborne and not climbing then 0 else 1

	weight(idle, "idle", wi * ground)
	weight(walk, "walk", ww * ground)
	weight(run, "run", wr * ground)
	weight(jump, "jump", 1 - ground)
	rate(walk, "walkRate", math.clamp(s / walkAt, 0.6, 1.6))
	rate(run, "runRate", math.clamp(s / runAt, 0.6, 2.5))
end)
