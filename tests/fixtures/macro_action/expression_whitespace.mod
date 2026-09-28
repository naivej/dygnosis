// inventory: spaces inside replacement do not erase the expression tokens
@#define rhs = " y + 1 "
var y;
model;
y=@{rhs};
end;
