"use client";

import React from "react";
import { useSidebar } from "@/components/Sidebar/SidebarProvider";

interface MainContentProps {
    children: React.ReactNode;
}

const MainContent: React.FC<MainContentProps> = ({ children }) => {
    const { isCollapsed, sidebarWidth, sidebarDragging } = useSidebar();

    return (
        <main
            className={`flex-1 ${sidebarDragging ? "" : "transition-all duration-300"} ${
                isCollapsed ? "ml-16" : ""
            }`}
            style={isCollapsed ? undefined : { marginLeft: sidebarWidth }}
        >
            <div className="pl-8">{children}</div>
        </main>
    );
};

export default MainContent;
