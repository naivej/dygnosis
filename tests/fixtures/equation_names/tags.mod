var y;
model;
    # x = 1;
    [static]
    y = 1;
    [name='']
    y = y(-1);
    [dynamic, group='g']
    y = y(-1);
end;
