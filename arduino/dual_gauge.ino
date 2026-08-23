#define A1_PIN 11
#define A2_PIN 10

String cmd;
int sepIdx;
String aChStr;
String aChValStr;

int a1Val;
int a1Curr;
int a2Val;
int a2Curr;

// the setup function runs once when you press reset or power the board
void setup() {
  // initialize digital pin LED_BUILTIN as an output.
  pinMode(LED_BUILTIN, OUTPUT);
  pinMode(A1_PIN, OUTPUT);
  pinMode(A2_PIN, OUTPUT);

  Serial.begin(9600);
  Serial.println("Type command: A=[0-100],[0-100] or A[1|2]=[0-100]");
}

// the loop function runs over and over again forever
void loop() {
  if (Serial.available()){
    digitalWrite(LED_BUILTIN, HIGH);
    cmd = Serial.readStringUntil("\n");
    cmd.trim();
    sepIdx = cmd.indexOf('=');
    if (sepIdx == -1) {
      digitalWrite(LED_BUILTIN, LOW);
      return;
    }
    aChStr = cmd.substring(0, sepIdx);
    aChValStr = cmd.substring(sepIdx+1);

    if (aChStr == "A") {
      sepIdx = aChValStr.indexOf(',');
      if (sepIdx == -1) {
        digitalWrite(LED_BUILTIN, LOW);
        return;
      }
      a1Val = aChValStr.substring(0, sepIdx).toInt();
      a2Val = aChValStr.substring(sepIdx+1).toInt();
      a1Val = map(a1Val, 0, 100, 0, 255);
      a2Val = map(a2Val, 0, 100, 0, 255);
    } else if(aChStr == "A1") {
      a1Val = aChValStr.toInt();
      a1Val = map(a1Val, 0, 100, 0, 255);
    } else if (aChStr == "A2") {
      a2Val = aChValStr.toInt();
      a2Val = map(a2Val, 0, 100, 0, 255);
    }
    digitalWrite(LED_BUILTIN, LOW);
  } else {
    if (a1Curr < a1Val) {
      a1Curr +=1;
    } else if (a1Curr > a1Val) {
      a1Curr -=1;
    }
    if (a2Curr < a2Val) {
      a2Curr +=1;
    } else if (a2Curr > a2Val) {
      a2Curr -=1;
    }
    analogWrite(A1_PIN, a1Curr);
    analogWrite(A2_PIN, a2Curr);
    delay(10);
  }
}