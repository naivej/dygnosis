// inventory: quoted macro text supplies a full equation expression
@#define rhs = "y+1"
var y;
model;
y=@{rhs};
end;
