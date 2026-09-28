// inventory: a macro string supplies a source identifier without quotes
@#define bad = "zz"
var y;
model;
y = @{bad};
end;
