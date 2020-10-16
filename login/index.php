<!DOCTYPE html>
<html lang="en">
<head>
	<title>Login V6</title>
	<meta charset="UTF-8">
	<meta name="viewport" content="width=device-width, initial-scale=1">
<!--===============================================================================================-->	
	<link rel="icon" type="image/png" href="images/icons/favicon.ico"/>
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="vendor/bootstrap/css/bootstrap.min.css">
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="fonts/font-awesome-4.7.0/css/font-awesome.min.css">
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="fonts/iconic/css/material-design-iconic-font.min.css">
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="vendor/animate/animate.css">
<!--===============================================================================================-->	
	<link rel="stylesheet" type="text/css" href="vendor/css-hamburgers/hamburgers.min.css">
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="vendor/animsition/css/animsition.min.css">
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="vendor/select2/select2.min.css">
<!--===============================================================================================-->	
	<link rel="stylesheet" type="text/css" href="vendor/daterangepicker/daterangepicker.css">
<!--===============================================================================================-->
	<link rel="stylesheet" type="text/css" href="css/util.css">
	<link rel="stylesheet" type="text/css" href="css/main.css">
<!--===============================================================================================-->
	<script src="js/md5.min.js"></script>
	<style>
		#overlay {
			position: fixed; /* Sit on top of the page content */
			display: block; /* Hidden by default */
			width: 100%; /* Full width (cover the whole page) */
			height: 100%; /* Full height (cover the whole page) */
			top: 0;
			left: 0;
			right: 0;
			bottom: 0;
			background-color: rgba(0,0,0,0.5); /* Black background with opacity */
			z-index: 2; /* Specify a stack order in case you're using a different order for other elements */
			cursor: pointer; /* Add a pointer on hover */
		}
		#connecting{
			position: absolute;
			top: 50%;
			left: 50%;
			font-size: 50px;
			color: white;
			transform: translate(-50%,-50%);
			-ms-transform: translate(-50%,-50%);
		}
	</style>
</head>
<body>
	<div id="overlay"><div id="connecting">Connecting ...</div></div>
	<div class="limiter">
		<div class="container-login100">
			<div class="wrap-login100 p-t-85 p-b-20">
				<form method=POST class="login100-form validate-form">
					<span class="login100-form-title p-b-70">
						Welcome
					</span>
					<span class="login100-form-avatar">
						<img src="images/avatar-01.png" alt="AVATAR">
					</span>
					
					<span style="color: red; display: none" id="wrong" name="wrong">
						Wrong username and/or password. Please try again.
					</span>
					<span style="color: red; display: none" id="not_exists" name="not_exists">
						The given username does not exist. Please sign up.
					</span>
					<div class="wrap-input100 validate-input m-t-85 m-b-35" data-validate = "Enter username">
						<input class="input100" type="text" id="username" name="username">
						<span class="focus-input100" data-placeholder="Username"></span>
					</div>

					<div class="wrap-input100 validate-input m-b-50" data-validate="Enter password">
						<input class="input100" type="password" id="password" name="password">
						<span class="focus-input100" data-placeholder="Password"></span>
					</div>

					<div class="container-login100-form-btn">
						<input type="submit" class="login100-form-btn" id="login" name="login" value="Login">
					</div>
					<br>
					<div class="container-login100-form-btn">
						<input type="button" class="login100-form-btn sign-up-btn" id="sign" name="sign" value="Sign Up" onclick="signup()">
					</div>

					<!-- <ul class="login-more p-t-190">
						<li class="m-b-8">
							<span class="txt1">
								Forgot
							</span>

							<a href="#" class="txt2">
								Username / Password?
							</a>
						</li>

						<li>
							<span class="txt1">
								Don’t have an account?
							</span>

							<a href="#" class="txt2">
								Sign up
							</a>
						</li>
					</ul> -->
				</form>
			</div>
		</div>
	</div>
	

	<div id="dropDownSelect1"></div>
	
<!--===============================================================================================-->
	<script src="vendor/jquery/jquery-3.2.1.min.js"></script>
<!--===============================================================================================-->
	<script src="vendor/animsition/js/animsition.min.js"></script>
<!--===============================================================================================-->
	<script src="vendor/bootstrap/js/popper.js"></script>
	<script src="vendor/bootstrap/js/bootstrap.min.js"></script>
<!--===============================================================================================-->
	<script src="vendor/select2/select2.min.js"></script>
<!--===============================================================================================-->
	<script src="vendor/daterangepicker/moment.min.js"></script>
	<script src="vendor/daterangepicker/daterangepicker.js"></script>
<!--===============================================================================================-->
	<script src="vendor/countdowntime/countdowntime.js"></script>
<!--===============================================================================================-->
	<script src="js/main.js"></script>

	<script type="text/javascript">
		const text = document.getElementById('text');
		const uri = "<?php 
			$cnf = json_decode(file_get_contents('./server.json'), true);
			$server = $cnf["host"] . ':' . $cnf["port"];
			echo 'ws://' . $server . '/wb';
		?>";
		var connected = false;

		var ws;

		function message(data) {
			var msg = JSON.parse(data);
			if (msg.type == 'login') {
				window.location.replace("http://<?php echo $server; ?>?user=" + msg.name + "&color=" + msg.color);
			} else if (msg.type == 'wrong') {
				wrong.style.display = 'inline';
			} else if (msg.type == 'not_exists') {
				not_exists.style.display = 'inline';
			}
		}

		function connect() {
			ws = new WebSocket(uri);
			ws.onopen = function() {
				connected = true;
				off();
			};

			ws.onmessage = function(msg) {
				message(msg.data);
			};

			ws.onclose = function() {
				connected = false;
				on();
				setTimeout(connect(), 1000);
			};
		}

		connect();

		function on() {
			document.getElementById("overlay").style.display = "block";
		}

		function off() {
			document.getElementById("overlay").style.display = "none";
		}
	</script>

</body>
</html>